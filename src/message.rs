use serde::{Deserialize, Serialize};

/// One WebSocket message to the overlay. Serializes as `{"type": "...", "data": ...}`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum OverlayMessage {
    Summoner(RiotId),
    Ranked(RankedSummary),
    Matches(MatchHistory),
}

impl OverlayMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Summoner(_) => "summoner",
            Self::Ranked(_) => "ranked",
            Self::Matches(_) => "matches",
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RiotId {
    pub game_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_line: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct Tier(pub String);

const DIVISION_LP: i64 = 100;
const TIER_LP: i64 = 4 * DIVISION_LP;
/// MASTER, GRANDMASTER and CHALLENGER start where DIAMOND I with 100 LP ends.
const APEX_FLOOR: i64 = 7 * TIER_LP;

/// The place of a tier on the ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TierLadder {
    /// A tier with four divisions. The value is the ladder points of division IV with 0 LP.
    Divided(i64),
    /// MASTER, GRANDMASTER and CHALLENGER share one LP range above DIAMOND I.
    Apex,
}

impl Tier {
    /// Returns `None` for values that are not a tier, for example "NONE" for unranked players.
    fn ladder(&self) -> Option<TierLadder> {
        match self.0.as_str() {
            "IRON" => Some(TierLadder::Divided(0)),
            "BRONZE" => Some(TierLadder::Divided(TIER_LP)),
            "SILVER" => Some(TierLadder::Divided(2 * TIER_LP)),
            "GOLD" => Some(TierLadder::Divided(3 * TIER_LP)),
            "PLATINUM" => Some(TierLadder::Divided(4 * TIER_LP)),
            "EMERALD" => Some(TierLadder::Divided(5 * TIER_LP)),
            "DIAMOND" => Some(TierLadder::Divided(6 * TIER_LP)),
            "MASTER" | "GRANDMASTER" | "CHALLENGER" => Some(TierLadder::Apex),
            _ => None,
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Division {
    #[serde(rename = "I")]
    One,
    #[serde(rename = "II")]
    Two,
    #[serde(rename = "III")]
    Three,
    #[serde(rename = "IV")]
    Four,
}

impl Division {
    /// Returns `None` for values without a division, for example "NA" for apex tiers.
    pub fn from_lcu(value: &str) -> Option<Self> {
        match value {
            "I" => Some(Self::One),
            "II" => Some(Self::Two),
            "III" => Some(Self::Three),
            "IV" => Some(Self::Four),
            _ => None,
        }
    }

    fn ladder_offset(self) -> i64 {
        match self {
            Self::Four => 0,
            Self::Three => DIVISION_LP,
            Self::Two => 2 * DIVISION_LP,
            Self::One => 3 * DIVISION_LP,
        }
    }
}

/// The position on the ranked ladder in LP, counted from IRON IV with 0 LP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LadderPoints(i64);

impl LadderPoints {
    /// Returns `None` for an unknown tier or for a tier with divisions but without a division.
    pub fn new(tier: &Tier, division: Option<Division>, league_points: i64) -> Option<Self> {
        match (tier.ladder()?, division) {
            (TierLadder::Apex, _) => Some(Self(APEX_FLOOR + league_points)),
            (TierLadder::Divided(floor), Some(division)) => {
                Some(Self(floor + division.ladder_offset() + league_points))
            }
            (TierLadder::Divided(_), None) => None,
        }
    }

    /// Returns the LP gained since `earlier`. A loss of LP is negative.
    pub fn since(self, earlier: LadderPoints) -> i64 {
        self.0 - earlier.0
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub wins: i64,
    pub losses: i64,
}

impl Record {
    pub fn since(self, baseline: Record) -> Record {
        Record {
            wins: self.wins - baseline.wins,
            losses: self.losses - baseline.losses,
        }
    }

    pub fn games(self) -> i64 {
        self.wins + self.losses
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LpChange {
    /// Not present before the first game in this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_game: Option<i64>,
    pub session_total: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RankedSummary {
    pub tier: Tier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub division: Option<Division>,
    pub league_points: i64,
    #[serde(flatten)]
    pub season: Record,
    pub session: Record,
    /// Not present when the start or the current rank is not on the ladder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_change: Option<LpChange>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(transparent)]
pub struct ChampionId(pub u32);

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MatchResult {
    Win,
    Loss,
    Remake,
}

impl MatchResult {
    /// Returns `None` for a remake, because a remake does not end or extend a streak.
    fn streak_kind(self) -> Option<StreakKind> {
        match self {
            Self::Win => Some(StreakKind::Win),
            Self::Loss => Some(StreakKind::Loss),
            Self::Remake => None,
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    pub champion_id: ChampionId,
    pub result: MatchResult,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreakKind {
    Win,
    Loss,
}

/// The number of the newest games in a row with the same result.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streak {
    pub kind: StreakKind,
    pub count: usize,
}

impl Streak {
    /// Reads `results` from the newest game. Returns `None` when there is no win or loss.
    pub fn from_results(results: impl IntoIterator<Item = MatchResult>) -> Option<Self> {
        let mut decided = results.into_iter().filter_map(MatchResult::streak_kind);
        let kind = decided.next()?;
        let count = 1 + decided.take_while(|next| *next == kind).count();
        Some(Self { kind, count })
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MatchHistory {
    pub games: Vec<MatchSummary>,
    pub streak: Option<Streak>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use MatchResult::{Loss, Remake, Win};
    use serde_json::json;

    fn to_json(message: &OverlayMessage) -> serde_json::Value {
        serde_json::to_value(message).expect("message serializes")
    }

    #[test]
    fn summoner_message_is_tagged() {
        let message = OverlayMessage::Summoner(RiotId {
            game_name: "PlayerOne".to_string(),
            tag_line: Some("EUW".to_string()),
        });

        assert_eq!(
            to_json(&message),
            json!({"type": "summoner", "data": {"gameName": "PlayerOne", "tagLine": "EUW"}})
        );
    }

    #[test]
    fn missing_tag_line_is_omitted() {
        let message = OverlayMessage::Summoner(RiotId {
            game_name: "PlayerOne".to_string(),
            tag_line: None,
        });

        assert_eq!(to_json(&message)["data"], json!({"gameName": "PlayerOne"}));
    }

    #[test]
    fn ranked_message_has_flat_season_record() {
        let message = OverlayMessage::Ranked(RankedSummary {
            tier: Tier("GOLD".to_string()),
            division: Some(Division::Two),
            league_points: 57,
            season: Record {
                wins: 34,
                losses: 29,
            },
            session: Record { wins: 2, losses: 1 },
            lp_change: Some(LpChange {
                last_game: Some(-18),
                session_total: 42,
            }),
        });

        assert_eq!(
            to_json(&message),
            json!({"type": "ranked", "data": {
                "tier": "GOLD",
                "division": "II",
                "leaguePoints": 57,
                "wins": 34,
                "losses": 29,
                "session": {"wins": 2, "losses": 1},
                "lpChange": {"lastGame": -18, "sessionTotal": 42}
            }})
        );
    }

    #[test]
    fn missing_last_game_is_omitted() {
        let change = LpChange {
            last_game: None,
            session_total: 0,
        };

        assert_eq!(
            serde_json::to_value(change).expect("change serializes"),
            json!({"sessionTotal": 0})
        );
    }

    fn ladder(tier: &str, division: Option<Division>, league_points: i64) -> LadderPoints {
        LadderPoints::new(&Tier(tier.to_string()), division, league_points)
            .expect("rank is on the ladder")
    }

    #[test]
    fn lp_change_in_same_division() {
        let before = ladder("GOLD", Some(Division::Two), 57);
        let after = ladder("GOLD", Some(Division::Two), 78);

        assert_eq!(after.since(before), 21);
    }

    #[test]
    fn division_promotion_counts_lp_across_divisions() {
        let before = ladder("SILVER", Some(Division::Four), 85);
        let after = ladder("SILVER", Some(Division::Three), 6);

        assert_eq!(after.since(before), 21);
    }

    #[test]
    fn tier_promotion_counts_lp_across_tiers() {
        let before = ladder("GOLD", Some(Division::One), 90);
        let after = ladder("PLATINUM", Some(Division::Four), 12);

        assert_eq!(after.since(before), 22);
    }

    #[test]
    fn demotion_is_negative() {
        let before = ladder("PLATINUM", Some(Division::Four), 5);
        let after = ladder("GOLD", Some(Division::One), 82);

        assert_eq!(after.since(before), -23);
    }

    #[test]
    fn diamond_one_to_master_continues_the_ladder() {
        let before = ladder("DIAMOND", Some(Division::One), 85);
        let after = ladder("MASTER", None, 0);

        assert_eq!(after.since(before), 15);
    }

    #[test]
    fn apex_tiers_use_raw_lp() {
        let before = ladder("MASTER", None, 480);
        let after = ladder("GRANDMASTER", None, 502);

        assert_eq!(after.since(before), 22);
    }

    #[test]
    fn unranked_or_missing_division_is_not_on_the_ladder() {
        assert_eq!(LadderPoints::new(&Tier("NONE".to_string()), None, 0), None);
        assert_eq!(LadderPoints::new(&Tier("GOLD".to_string()), None, 50), None);
    }

    #[test]
    fn match_results_are_lowercase() {
        let message = OverlayMessage::Matches(MatchHistory {
            games: vec![MatchSummary {
                champion_id: ChampionId(157),
                result: MatchResult::Remake,
            }],
            streak: Some(Streak {
                kind: StreakKind::Loss,
                count: 2,
            }),
        });

        assert_eq!(
            to_json(&message),
            json!({"type": "matches", "data": {
                "games": [{"championId": 157, "result": "remake"}],
                "streak": {"kind": "loss", "count": 2}
            }})
        );
    }

    #[test]
    fn missing_streak_is_null() {
        let message = OverlayMessage::Matches(MatchHistory {
            games: Vec::new(),
            streak: None,
        });

        assert_eq!(
            to_json(&message)["data"],
            json!({"games": [], "streak": null})
        );
    }

    fn streak(kind: StreakKind, count: usize) -> Option<Streak> {
        Some(Streak { kind, count })
    }

    #[test]
    fn streak_of_only_wins_counts_all_games() {
        assert_eq!(
            Streak::from_results([Win, Win, Win]),
            streak(StreakKind::Win, 3)
        );
    }

    #[test]
    fn streak_ends_at_the_first_other_result() {
        assert_eq!(
            Streak::from_results([Loss, Loss, Win, Loss]),
            streak(StreakKind::Loss, 2)
        );
        assert_eq!(
            Streak::from_results([Win, Loss, Loss]),
            streak(StreakKind::Win, 1)
        );
    }

    #[test]
    fn streak_skips_remakes() {
        assert_eq!(
            Streak::from_results([Remake, Win, Remake, Win, Loss]),
            streak(StreakKind::Win, 2)
        );
    }

    #[test]
    fn empty_history_has_no_streak() {
        assert_eq!(Streak::from_results([]), None);
    }

    #[test]
    fn only_remakes_have_no_streak() {
        assert_eq!(Streak::from_results([Remake, Remake]), None);
    }

    #[test]
    fn division_na_is_absent() {
        assert_eq!(Division::from_lcu("NA"), None);
        assert_eq!(Division::from_lcu("III"), Some(Division::Three));
    }
}

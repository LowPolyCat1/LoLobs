use serde::{Deserialize, Serialize};

/// One WebSocket message to the overlay. Serializes as `{"type": "...", "data": ...}`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum OverlayMessage {
    Summoner(RiotId),
    Ranked(RankedSummary),
    Matches(Vec<MatchSummary>),
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

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    pub champion_id: ChampionId,
    pub result: MatchResult,
}

#[cfg(test)]
mod tests {
    use super::*;
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
        });

        assert_eq!(
            to_json(&message),
            json!({"type": "ranked", "data": {
                "tier": "GOLD",
                "division": "II",
                "leaguePoints": 57,
                "wins": 34,
                "losses": 29,
                "session": {"wins": 2, "losses": 1}
            }})
        );
    }

    #[test]
    fn match_results_are_lowercase() {
        let message = OverlayMessage::Matches(vec![MatchSummary {
            champion_id: ChampionId(157),
            result: MatchResult::Remake,
        }]);

        assert_eq!(
            to_json(&message),
            json!({"type": "matches", "data": [{"championId": 157, "result": "remake"}]})
        );
    }

    #[test]
    fn division_na_is_absent() {
        assert_eq!(Division::from_lcu("NA"), None);
        assert_eq!(Division::from_lcu("III"), Some(Division::Three));
    }
}

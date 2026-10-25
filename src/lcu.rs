use crate::config::{MatchLimit, Queue};
use crate::message::{
    ChampionId, Division, MatchResult, MatchSummary, OverlayMessage, RankedSummary, Record, RiotId,
    Tier,
};
use crate::snapshot::Outbox;
use irelia::ws::{
    LcuWebSocket,
    types::{EventData, EventKind},
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;

type ConcreteLcuClient = irelia::rest::LcuClient<irelia::requests::RequestClientType>;

const SUMMONER_URI: &str = "/lol-summoner/v1/current-summoner";
const RANKED_STATS_URI: &str = "/lol-ranked/v1/current-ranked-stats";
const MATCH_HISTORY_URI: &str = "/lol-match-history/v1/products/lol/current-summoner/matches";
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(transparent)]
struct SummonerId(u64);

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(transparent)]
struct QueueId(u32);

/// The queue that the overlay shows: its name in ranked stats and its ID in match history.
#[derive(Clone, Copy, Debug)]
struct RankedQueue {
    queue_type: &'static str,
    queue_id: QueueId,
}

impl RankedQueue {
    const fn from_config(queue: Queue) -> Self {
        match queue {
            Queue::Solo => Self {
                queue_type: "RANKED_SOLO_5x5",
                queue_id: QueueId(420),
            },
            Queue::Flex => Self {
                queue_type: "RANKED_FLEX_SR",
                queue_id: QueueId(440),
            },
        }
    }
}

/// The data that the overlay shows: the queue and the number of recent games.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    queue: RankedQueue,
    match_limit: MatchLimit,
}

impl Selection {
    pub const fn new(queue: Queue, match_limit: MatchLimit) -> Self {
        Self {
            queue: RankedQueue::from_config(queue),
            match_limit,
        }
    }
}

#[derive(Debug)]
struct LcuDataError {
    uri: &'static str,
    source: serde_json::Error,
}

impl fmt::Display for LcuDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Cannot read the LCU data from {}: {}",
            self.uri, self.source
        )
    }
}

fn parse<T: DeserializeOwned>(
    data: &serde_json::Value,
    uri: &'static str,
) -> Result<T, LcuDataError> {
    T::deserialize(data).map_err(|source| LcuDataError { uri, source })
}

fn log_data_error(err: &LcuDataError) {
    tracing::warn!(target: "lcu_monitor", "{}", err);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuAccount {
    summoner_id: Option<SummonerId>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuSummoner {
    game_name: String,
    tag_line: Option<String>,
}

impl LcuSummoner {
    fn into_message(self) -> OverlayMessage {
        OverlayMessage::Summoner(RiotId {
            game_name: self.game_name,
            tag_line: self.tag_line,
        })
    }
}

#[derive(Deserialize)]
struct LcuRankedStats {
    queues: Vec<LcuQueue>,
}

impl LcuRankedStats {
    fn queue(&self, queue: RankedQueue) -> Option<&LcuQueue> {
        self.queues
            .iter()
            .find(|q| q.queue_type == queue.queue_type)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuQueue {
    queue_type: String,
    #[serde(default)]
    tier: Tier,
    division: Option<String>,
    #[serde(default)]
    league_points: i64,
    #[serde(default)]
    wins: i64,
    #[serde(default)]
    losses: i64,
}

impl LcuQueue {
    fn record(&self) -> Record {
        Record {
            wins: self.wins,
            losses: self.losses,
        }
    }

    fn summary(&self, session: Record) -> RankedSummary {
        RankedSummary {
            tier: self.tier.clone(),
            division: self.division.as_deref().and_then(Division::from_lcu),
            league_points: self.league_points,
            season: self.record(),
            session,
        }
    }
}

#[derive(Deserialize)]
struct LcuMatchHistory {
    games: LcuGameList,
}

#[derive(Deserialize)]
struct LcuGameList {
    games: Vec<LcuGame>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuGame {
    queue_id: QueueId,
    participants: Vec<LcuParticipant>,
}

impl LcuGame {
    // The match history only lists the current summoner as participant.
    fn summary(&self) -> Option<MatchSummary> {
        let participant = self.participants.first()?;
        Some(MatchSummary {
            champion_id: participant.champion_id,
            result: participant.stats.result(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuParticipant {
    champion_id: ChampionId,
    stats: LcuParticipantStats,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuParticipantStats {
    win: bool,
    #[serde(default)]
    game_ended_in_early_surrender: bool,
}

impl LcuParticipantStats {
    // The LCU flags a remake as an early surrender. A limit on the game duration
    // was rejected: it only guesses and can misclassify games.
    fn result(&self) -> MatchResult {
        match (self.game_ended_in_early_surrender, self.win) {
            (true, _) => MatchResult::Remake,
            (false, true) => MatchResult::Win,
            (false, false) => MatchResult::Loss,
        }
    }
}

fn recent_games(history: &LcuMatchHistory, selection: Selection) -> Vec<MatchSummary> {
    history
        .games
        .games
        .iter()
        .filter(|game| game.queue_id == selection.queue.queue_id)
        .filter_map(LcuGame::summary)
        .take(selection.match_limit.get())
        .collect()
}

#[derive(Default)]
struct SessionTracker {
    summoner_id: Option<SummonerId>,
    baseline: Option<Record>,
}

impl SessionTracker {
    fn switch_account(&mut self, id: Option<SummonerId>) -> bool {
        if self.summoner_id == id {
            return false;
        }
        self.summoner_id = id;
        self.baseline = None;
        true
    }

    fn session(&mut self, current: Record) -> Record {
        let baseline = *self.baseline.get_or_insert_with(|| {
            tracing::info!(target: "lcu_monitor", "New baseline established: {}W - {}L", current.wins, current.losses);
            current
        });
        current.since(baseline)
    }
}

fn ranked_summary(
    stats: &LcuRankedStats,
    queue: RankedQueue,
    tracker: &mut SessionTracker,
) -> Option<RankedSummary> {
    let queue = stats.queue(queue)?;
    let session = tracker.session(queue.record());
    Some(queue.summary(session))
}

type SharedState = Arc<Mutex<SessionTracker>>;

pub trait LcuSource {
    fn fetch(&self, uri: &str) -> impl Future<Output = Option<serde_json::Value>> + Send;
    fn subscribe(&mut self, handler: impl Fn(&EventData) + Send + 'static) -> Option<()>;
}

struct IreliaSource {
    client: ConcreteLcuClient,
    socket: Option<LcuWebSocket>,
}

impl IreliaSource {
    fn connect() -> Option<Self> {
        let request_client = irelia::requests::new();
        let client = irelia::rest::LcuClient::connect_with_request_client(&request_client).ok()?;
        Some(Self {
            client,
            socket: None,
        })
    }
}

impl LcuSource for IreliaSource {
    async fn fetch(&self, uri: &str) -> Option<serde_json::Value> {
        self.client.get::<serde_json::Value>(uri).await.ok()
    }

    fn subscribe(&mut self, handler: impl Fn(&EventData) + Send + 'static) -> Option<()> {
        let mut socket = LcuWebSocket::new();
        socket.subscribe_closure(EventKind::json_api_event(), move |event| handler(&event.2))?;
        self.socket = Some(socket);
        Some(())
    }
}

fn lock_state(state: &SharedState) -> MutexGuard<'_, SessionTracker> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn spawn_lcu_listener(outbox: Outbox, selection: Selection) {
    let state = Arc::new(Mutex::new(SessionTracker::default()));

    tokio::spawn(async move {
        tracing::info!("LCU Listener task spawned. Searching for active League client...");

        loop {
            run_irelia_cycle(&outbox, &state, selection).await;
            tokio::time::sleep(RETRY_DELAY).await;
        }
    });
}

async fn run_irelia_cycle(outbox: &Outbox, state: &SharedState, selection: Selection) {
    let Some(mut source) = IreliaSource::connect() else {
        return;
    };
    run_listener_cycle(&mut source, outbox, state, selection).await;
}

async fn run_listener_cycle(
    source: &mut impl LcuSource,
    outbox: &Outbox,
    state: &SharedState,
    selection: Selection,
) {
    let Some(summoner) = source.fetch(SUMMONER_URI).await else {
        return;
    };

    update_account(state, &summoner);

    // The irelia callback runs on its own thread without a tokio runtime, so events go
    // through a channel to this task, which can make async LCU requests.
    let (events_tx, events) = mpsc::unbounded_channel();
    let connection = source.subscribe(move |event| {
        let _ = events_tx.send(event.clone());
    });

    if connection.is_none() {
        return;
    }

    tracing::info!(target: "lcu_monitor", "Connected to LCU WebSocket.");

    send_initial_sync(source, outbox, state, selection).await;

    // A closed event channel only means that the LCU socket stopped. The cycle still
    // ends only when the client is gone.
    tokio::select! {
        () = wait_until_disconnected(source) => {}
        () = process_events(source, events, outbox, state, selection) => wait_until_disconnected(source).await,
    }

    tracing::warn!(target: "lcu_monitor", "LCU connection lost. Retrying...");
}

async fn process_events(
    source: &impl LcuSource,
    mut events: mpsc::UnboundedReceiver<EventData>,
    outbox: &Outbox,
    state: &SharedState,
    selection: Selection,
) {
    while let Some(event) = events.recv().await {
        handle_lcu_event(&event, outbox, state, selection);
        refresh_matches_after_ranked_update(source, &event, outbox, selection).await;
    }
}

async fn refresh_matches_after_ranked_update(
    source: &impl LcuSource,
    event: &EventData,
    outbox: &Outbox,
    selection: Selection,
) {
    if event.uri != RANKED_STATS_URI {
        return;
    }
    sync_recent_matches(source, outbox, selection).await;
}

fn update_account(state: &SharedState, summoner: &serde_json::Value) {
    let new_id = parse::<LcuAccount>(summoner, SUMMONER_URI)
        .inspect_err(log_data_error)
        .ok()
        .and_then(|account| account.summoner_id);
    if !lock_state(state).switch_account(new_id) {
        return;
    }
    tracing::info!(target: "lcu_monitor", "Account detected: ID {:?}", new_id);
}

fn handle_lcu_event(
    payload: &EventData,
    outbox: &Outbox,
    state: &SharedState,
    selection: Selection,
) {
    match payload.uri.as_str() {
        RANKED_STATS_URI => publish_ranked_stats(&payload.data, outbox, state, selection),
        SUMMONER_URI => publish_summoner(&payload.data, outbox),
        MATCH_HISTORY_URI => publish_matches(&payload.data, outbox, selection),
        _ => {}
    }
}

fn publish_summoner(data: &serde_json::Value, outbox: &Outbox) {
    let Ok(summoner) = parse::<LcuSummoner>(data, SUMMONER_URI).inspect_err(log_data_error) else {
        return;
    };
    publish(outbox, &summoner.into_message());
}

fn publish_ranked_stats(
    data: &serde_json::Value,
    outbox: &Outbox,
    state: &SharedState,
    selection: Selection,
) {
    let Ok(stats) = parse::<LcuRankedStats>(data, RANKED_STATS_URI).inspect_err(log_data_error)
    else {
        return;
    };
    let Some(summary) = ranked_summary(&stats, selection.queue, &mut lock_state(state)) else {
        return;
    };
    publish(outbox, &OverlayMessage::Ranked(summary));
}

fn publish_matches(data: &serde_json::Value, outbox: &Outbox, selection: Selection) {
    let Ok(history) = parse::<LcuMatchHistory>(data, MATCH_HISTORY_URI).inspect_err(log_data_error)
    else {
        return;
    };
    let games = recent_games(&history, selection);
    publish(outbox, &OverlayMessage::Matches(games));
}

fn publish(outbox: &Outbox, message: &OverlayMessage) {
    if let Err(err) = outbox.publish(message) {
        tracing::error!(target: "overlay_update", "Cannot serialize the {} message: {}", message.name(), err);
        return;
    }
    tracing::info!(target: "overlay_update", "Pushing {} update", message.name());
}

async fn send_initial_sync(
    source: &impl LcuSource,
    outbox: &Outbox,
    state: &SharedState,
    selection: Selection,
) {
    sync_summoner(source, outbox).await;
    sync_ranked_stats(source, outbox, state, selection).await;
    sync_recent_matches(source, outbox, selection).await;
}

async fn sync_summoner(source: &impl LcuSource, outbox: &Outbox) {
    let Some(profile) = source.fetch(SUMMONER_URI).await else {
        return;
    };
    publish_summoner(&profile, outbox);
}

async fn sync_ranked_stats(
    source: &impl LcuSource,
    outbox: &Outbox,
    state: &SharedState,
    selection: Selection,
) {
    let Some(stats) = source.fetch(RANKED_STATS_URI).await else {
        return;
    };
    publish_ranked_stats(&stats, outbox, state, selection);
}

async fn sync_recent_matches(source: &impl LcuSource, outbox: &Outbox, selection: Selection) {
    let Some(matches) = source.fetch(MATCH_HISTORY_URI).await else {
        return;
    };
    publish_matches(&matches, outbox, selection);
}

async fn wait_until_disconnected(source: &impl LcuSource) {
    while source.fetch(SUMMONER_URI).await.is_some() {
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SUMMONER: &str = include_str!("../tests/fixtures/summoner.json");
    const RANKED_STATS: &str = include_str!("../tests/fixtures/ranked_stats.json");
    const MATCHES: &str = include_str!("../tests/fixtures/matches.json");
    const RANKED_STATS_AFTER_GAMES: &str =
        include_str!("../tests/fixtures/ranked_stats_after_games.json");
    const RANKED_STATS_FLEX_ONLY: &str =
        include_str!("../tests/fixtures/ranked_stats_flex_only.json");
    const RANKED_STATS_MASTER: &str = include_str!("../tests/fixtures/ranked_stats_master.json");
    const RANKED_STATS_NO_RECORD: &str =
        include_str!("../tests/fixtures/ranked_stats_no_record.json");

    // The e2e tests feed these messages to the overlay. The contract tests below
    // check that the backend sends the same messages for the LCU fixtures.
    const E2E_SUMMONER: &str = include_str!("../e2e/fixtures/summoner.json");
    const E2E_RANKED: &str = include_str!("../e2e/fixtures/ranked.json");
    const E2E_RANKED_MASTER: &str = include_str!("../e2e/fixtures/ranked_master.json");
    const E2E_MATCHES: &str = include_str!("../e2e/fixtures/matches.json");

    struct MockSource {
        responses: HashMap<&'static str, Value>,
        summoner_checks_left: AtomicUsize,
        events: Vec<EventData>,
    }

    impl MockSource {
        fn running(summoner: Value, ranked_stats: Value, summoner_checks: usize) -> Self {
            let responses = HashMap::from([
                (SUMMONER_URI, summoner),
                (RANKED_STATS_URI, ranked_stats),
                (MATCH_HISTORY_URI, fixture(MATCHES)),
            ]);
            Self {
                responses,
                summoner_checks_left: AtomicUsize::new(summoner_checks),
                events: Vec::new(),
            }
        }

        fn not_running() -> Self {
            Self {
                responses: HashMap::new(),
                summoner_checks_left: AtomicUsize::new(0),
                events: Vec::new(),
            }
        }

        fn with_events(mut self, events: Vec<EventData>) -> Self {
            self.events = events;
            self
        }

        fn take_summoner_check(&self) -> bool {
            self.summoner_checks_left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
        }
    }

    impl LcuSource for MockSource {
        async fn fetch(&self, uri: &str) -> Option<Value> {
            if uri == SUMMONER_URI && !self.take_summoner_check() {
                return None;
            }
            self.responses.get(uri).cloned()
        }

        fn subscribe(&mut self, handler: impl Fn(&EventData) + Send + 'static) -> Option<()> {
            self.events.iter().for_each(handler);
            Some(())
        }
    }

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is valid JSON")
    }

    fn summoner_with_id(id: u64) -> Value {
        let mut summoner = fixture(SUMMONER);
        summoner["summonerId"] = json!(id);
        summoner
    }

    fn ranked_with_record(wins: i64, losses: i64) -> Value {
        let mut stats = fixture(RANKED_STATS);
        stats["queues"][0]["wins"] = json!(wins);
        stats["queues"][0]["losses"] = json!(losses);
        stats
    }

    fn event(uri: &str, data: Value) -> EventData {
        EventData {
            data,
            event_type: "Update".to_string(),
            uri: uri.to_string(),
        }
    }

    const SOLO: Selection = Selection::new(Queue::Solo, MatchLimit::DEFAULT);

    fn with_limit(queue: Queue, limit: u64) -> Selection {
        Selection::new(queue, MatchLimit::try_from(limit).expect("limit is valid"))
    }

    fn new_state() -> SharedState {
        Arc::new(Mutex::new(SessionTracker::default()))
    }

    fn drain(rx: &mut tokio::sync::broadcast::Receiver<String>) -> Vec<Value> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .map(|msg| serde_json::from_str(&msg).expect("broadcast is valid JSON"))
            .collect()
    }

    async fn run_cycle(source: &mut MockSource, state: &SharedState) -> Vec<Value> {
        run_cycle_with_outbox(source, state, &Outbox::new(32)).await
    }

    async fn run_cycle_with_outbox(
        source: &mut MockSource,
        state: &SharedState,
        outbox: &Outbox,
    ) -> Vec<Value> {
        let mut rx = outbox.subscribe();
        let cycle = run_listener_cycle(source, outbox, state, SOLO);
        tokio::time::timeout(Duration::from_secs(600), cycle)
            .await
            .expect("cycle ends after disconnect");
        drain(&mut rx)
    }

    fn message_json(message: &OverlayMessage) -> Value {
        serde_json::to_value(message).expect("message serializes")
    }

    fn summoner_message(text: &str) -> OverlayMessage {
        parse::<LcuSummoner>(&fixture(text), SUMMONER_URI)
            .expect("summoner fixture parses")
            .into_message()
    }

    fn ranked_from(text: &str, tracker: &mut SessionTracker) -> Option<RankedSummary> {
        let stats = parse::<LcuRankedStats>(&fixture(text), RANKED_STATS_URI)
            .expect("ranked fixture parses");
        ranked_summary(&stats, SOLO.queue, tracker)
    }

    fn history(text: &str) -> LcuMatchHistory {
        parse(&fixture(text), MATCH_HISTORY_URI).expect("match fixture parses")
    }

    fn tracker_with_baseline(wins: i64, losses: i64) -> SessionTracker {
        SessionTracker {
            summoner_id: None,
            baseline: Some(Record { wins, losses }),
        }
    }

    fn game(champion_id: u32, result: MatchResult) -> MatchSummary {
        MatchSummary {
            champion_id: ChampionId(champion_id),
            result,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cycle_returns_when_client_not_running() {
        let messages = run_cycle(&mut MockSource::not_running(), &new_state()).await;

        assert!(messages.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn initial_sync_sends_summoner_ranked_and_matches() {
        let mut source = MockSource::running(fixture(SUMMONER), fixture(RANKED_STATS), 3);

        let messages = run_cycle(&mut source, &new_state()).await;

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0], message_json(&summoner_message(SUMMONER)));
        assert_eq!(messages[1]["type"], "ranked");
        assert_eq!(
            messages[1]["data"]["session"],
            json!({"wins": 0, "losses": 0})
        );
        assert_eq!(messages[2]["type"], "matches");
    }

    #[tokio::test(start_paused = true)]
    async fn initial_sync_fills_snapshot_with_broadcast_messages() {
        let outbox = Outbox::new(32);
        let mut source = MockSource::running(fixture(SUMMONER), fixture(RANKED_STATS), 3);

        let messages = run_cycle_with_outbox(&mut source, &new_state(), &outbox).await;

        let snapshot: Vec<Value> = outbox
            .snapshot()
            .messages()
            .iter()
            .map(|msg| serde_json::from_str(msg).expect("snapshot is valid JSON"))
            .collect();
        assert_eq!(snapshot, messages);
    }

    #[tokio::test(start_paused = true)]
    async fn cycle_ends_when_client_disconnects() {
        let mut source = MockSource::running(fixture(SUMMONER), fixture(RANKED_STATS), 10);

        run_cycle(&mut source, &new_state()).await;

        assert!(!source.take_summoner_check());
    }

    #[tokio::test(start_paused = true)]
    async fn same_account_keeps_baseline() {
        let state = new_state();
        let mut first = MockSource::running(summoner_with_id(1), ranked_with_record(10, 5), 3);
        let mut second = MockSource::running(summoner_with_id(1), ranked_with_record(12, 6), 3);

        run_cycle(&mut first, &state).await;
        let messages = run_cycle(&mut second, &state).await;

        assert_eq!(
            messages[1]["data"]["session"],
            json!({"wins": 2, "losses": 1})
        );
    }

    #[tokio::test(start_paused = true)]
    async fn account_switch_resets_baseline() {
        let state = new_state();
        let mut first = MockSource::running(summoner_with_id(1), ranked_with_record(10, 5), 3);
        let mut second = MockSource::running(summoner_with_id(2), ranked_with_record(12, 6), 3);

        run_cycle(&mut first, &state).await;
        let messages = run_cycle(&mut second, &state).await;

        assert_eq!(
            messages[1]["data"]["session"],
            json!({"wins": 0, "losses": 0})
        );
    }

    #[test]
    fn summoner_keeps_only_the_riot_id() {
        assert_eq!(
            summoner_message(SUMMONER),
            OverlayMessage::Summoner(RiotId {
                game_name: "PlayerOne".to_string(),
                tag_line: Some("EUW".to_string()),
            })
        );
    }

    #[test]
    fn summoner_without_tag_line_has_no_tag_line() {
        let mut summoner = fixture(SUMMONER);
        summoner["tagLine"] = Value::Null;

        let message = parse::<LcuSummoner>(&summoner, SUMMONER_URI)
            .expect("summoner parses")
            .into_message();

        assert_eq!(
            message_json(&message)["data"],
            json!({"gameName": "PlayerOne"})
        );
    }

    #[test]
    fn invalid_summoner_names_the_uri() {
        let err = parse::<LcuSummoner>(&json!({"tagLine": "EUW"}), SUMMONER_URI)
            .err()
            .expect("summoner without game name fails");

        assert!(err.to_string().contains(SUMMONER_URI));
    }

    #[test]
    fn first_ranked_payload_sets_baseline() {
        let summary = ranked_from(RANKED_STATS, &mut SessionTracker::default());

        assert_eq!(
            summary,
            Some(RankedSummary {
                tier: Tier("GOLD".to_string()),
                division: Some(Division::Two),
                league_points: 57,
                season: Record {
                    wins: 34,
                    losses: 29
                },
                session: Record { wins: 0, losses: 0 },
            })
        );
    }

    #[test]
    fn apex_tier_has_no_division() {
        let summary = ranked_from(RANKED_STATS_MASTER, &mut SessionTracker::default())
            .expect("solo queue present");

        assert_eq!(summary.division, None);
    }

    #[test]
    fn later_ranked_payload_reports_deltas() {
        let mut tracker = SessionTracker::default();

        ranked_from(RANKED_STATS, &mut tracker);
        let summary =
            ranked_from(RANKED_STATS_AFTER_GAMES, &mut tracker).expect("solo queue present");

        assert_eq!(summary.session, Record { wins: 2, losses: 1 });
    }

    #[test]
    fn ranked_payload_without_solo_queue_is_ignored() {
        assert_eq!(
            ranked_from(RANKED_STATS_FLEX_ONLY, &mut SessionTracker::default()),
            None
        );
    }

    #[test]
    fn flex_queue_reads_the_flex_ranked_stats() {
        let stats = parse::<LcuRankedStats>(&fixture(RANKED_STATS_FLEX_ONLY), RANKED_STATS_URI)
            .expect("ranked fixture parses");
        let flex = Selection::new(Queue::Flex, MatchLimit::DEFAULT);

        let summary = ranked_summary(&stats, flex.queue, &mut SessionTracker::default());

        assert_eq!(summary.map(|s| s.tier), Some(Tier("SILVER".to_string())));
    }

    #[test]
    fn flex_queue_keeps_only_flex_games() {
        let games = recent_games(&history(MATCHES), with_limit(Queue::Flex, 20));

        assert_eq!(games, [game(81, MatchResult::Loss)]);
    }

    #[test]
    fn missing_wins_and_losses_count_as_zero() {
        let mut tracker = SessionTracker::default();

        ranked_from(RANKED_STATS_NO_RECORD, &mut tracker);
        let summary = ranked_from(RANKED_STATS, &mut tracker).expect("solo queue present");

        assert_eq!(
            summary.session,
            Record {
                wins: 34,
                losses: 29
            }
        );
    }

    #[test]
    fn match_history_keeps_recent_solo_queue_games() {
        let games = recent_games(&history(MATCHES), SOLO);

        assert_eq!(
            games,
            [
                game(103, MatchResult::Win),
                game(222, MatchResult::Loss),
                game(157, MatchResult::Remake),
                game(64, MatchResult::Win),
                game(25, MatchResult::Loss),
            ]
        );
    }

    #[test]
    fn match_history_respects_the_limit() {
        let games = recent_games(&history(MATCHES), with_limit(Queue::Solo, 2));

        assert_eq!(games.len(), 2);
    }

    #[test]
    fn game_without_participants_is_skipped() {
        let mut matches = fixture(MATCHES);
        matches["games"]["games"][0]["participants"] = json!([]);
        let history: LcuMatchHistory =
            parse(&matches, MATCH_HISTORY_URI).expect("match history parses");

        let games = recent_games(&history, SOLO);

        assert_eq!(games[0], game(222, MatchResult::Loss));
    }

    fn result_of(stats: Value) -> MatchResult {
        serde_json::from_value::<LcuParticipantStats>(stats)
            .expect("stats parse")
            .result()
    }

    #[test]
    fn early_surrender_is_a_remake_for_both_teams() {
        let lost = json!({"win": false, "gameEndedInEarlySurrender": true});
        let won = json!({"win": true, "gameEndedInEarlySurrender": true});

        assert_eq!(result_of(lost), MatchResult::Remake);
        assert_eq!(result_of(won), MatchResult::Remake);
    }

    #[test]
    fn short_game_without_early_surrender_is_not_a_remake() {
        let mut matches = fixture(MATCHES);
        matches["games"]["games"][0]["gameDuration"] = json!(200);
        let history: LcuMatchHistory =
            parse(&matches, MATCH_HISTORY_URI).expect("match history parses");

        let games = recent_games(&history, SOLO);

        assert_eq!(games[0], game(103, MatchResult::Win));
    }

    #[test]
    fn long_game_with_early_surrender_is_a_remake() {
        let mut matches = fixture(MATCHES);
        matches["games"]["games"][1]["gameDuration"] = json!(400);
        matches["games"]["games"][1]["participants"][0]["stats"]["gameEndedInEarlySurrender"] =
            json!(true);
        let history: LcuMatchHistory =
            parse(&matches, MATCH_HISTORY_URI).expect("match history parses");

        let games = recent_games(&history, SOLO);

        assert_eq!(games[1], game(222, MatchResult::Remake));
    }

    #[test]
    fn missing_early_surrender_flag_uses_win() {
        assert_eq!(result_of(json!({"win": true})), MatchResult::Win);
        assert_eq!(result_of(json!({"win": false})), MatchResult::Loss);
    }

    #[test]
    fn summoner_message_matches_e2e_fixture() {
        assert_eq!(
            message_json(&summoner_message(SUMMONER)),
            fixture(E2E_SUMMONER)
        );
    }

    #[test]
    fn ranked_message_matches_e2e_fixture() {
        let summary = ranked_from(RANKED_STATS, &mut tracker_with_baseline(32, 28))
            .expect("solo queue present");

        assert_eq!(
            message_json(&OverlayMessage::Ranked(summary)),
            fixture(E2E_RANKED)
        );
    }

    #[test]
    fn apex_ranked_message_matches_e2e_fixture() {
        let summary = ranked_from(RANKED_STATS_MASTER, &mut SessionTracker::default())
            .expect("solo queue present");

        assert_eq!(
            message_json(&OverlayMessage::Ranked(summary)),
            fixture(E2E_RANKED_MASTER)
        );
    }

    #[test]
    fn matches_message_matches_e2e_fixture() {
        let games = recent_games(&history(MATCHES), SOLO);

        assert_eq!(
            message_json(&OverlayMessage::Matches(games)),
            fixture(E2E_MATCHES)
        );
    }

    #[test]
    fn tracker_switch_to_other_account_resets_baseline() {
        let mut tracker = SessionTracker::default();
        tracker.switch_account(Some(SummonerId(1)));
        tracker.session(Record {
            wins: 10,
            losses: 5,
        });

        let switched = tracker.switch_account(Some(SummonerId(2)));

        assert!(switched);
        assert_eq!(
            tracker.session(Record {
                wins: 12,
                losses: 6
            }),
            Record { wins: 0, losses: 0 }
        );
    }

    #[test]
    fn tracker_same_account_keeps_baseline() {
        let mut tracker = SessionTracker::default();
        tracker.switch_account(Some(SummonerId(1)));
        tracker.session(Record {
            wins: 10,
            losses: 5,
        });

        let switched = tracker.switch_account(Some(SummonerId(1)));

        assert!(!switched);
        assert_eq!(
            tracker.session(Record {
                wins: 12,
                losses: 6
            }),
            Record { wins: 2, losses: 1 }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ranked_update_event_refreshes_match_history() {
        let ranked_event = event(RANKED_STATS_URI, fixture(RANKED_STATS_AFTER_GAMES));
        let mut source = MockSource::running(fixture(SUMMONER), fixture(RANKED_STATS), 3)
            .with_events(vec![ranked_event]);

        let messages = run_cycle(&mut source, &new_state()).await;

        assert_eq!(messages.len(), 5);
        assert_eq!(
            messages[3]["data"]["session"],
            json!({"wins": 2, "losses": 1})
        );
        assert_eq!(messages[4], messages[2]);
        assert_eq!(messages[4]["type"], "matches");
    }

    #[tokio::test(start_paused = true)]
    async fn other_events_do_not_refresh_match_history() {
        let summoner_event = event(SUMMONER_URI, fixture(SUMMONER));
        let mut source = MockSource::running(fixture(SUMMONER), fixture(RANKED_STATS), 3)
            .with_events(vec![summoner_event]);

        let messages = run_cycle(&mut source, &new_state()).await;

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[3], messages[0]);
        assert_eq!(messages[3]["type"], "summoner");
    }
}

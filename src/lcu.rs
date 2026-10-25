use crate::snapshot::{Outbox, Topic};
use irelia::ws::{
    LcuWebSocket,
    types::{EventData, EventKind},
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

type ConcreteLcuClient = irelia::rest::LcuClient<irelia::requests::RequestClientType>;

const SUMMONER_URI: &str = "/lol-summoner/v1/current-summoner";
const RANKED_STATS_URI: &str = "/lol-ranked/v1/current-ranked-stats";
const MATCH_HISTORY_URI: &str = "/lol-match-history/v1/products/lol/current-summoner/matches";
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SummonerId(u64);

impl SummonerId {
    fn from_summoner(summoner: &serde_json::Value) -> Option<Self> {
        summoner["summonerId"].as_u64().map(Self)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub wins: i64,
    pub losses: i64,
}

impl Record {
    fn from_queue(queue: &serde_json::Value) -> Self {
        Self {
            wins: queue["wins"].as_i64().unwrap_or(0),
            losses: queue["losses"].as_i64().unwrap_or(0),
        }
    }

    fn since(self, baseline: Record) -> Record {
        Record {
            wins: self.wins - baseline.wins,
            losses: self.losses - baseline.losses,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RankedStats {
    pub queues: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<Record>,
}

impl RankedStats {
    fn solo_queue_record(&self) -> Option<Record> {
        self.queues
            .iter()
            .find(|q| q["queueType"] == "RANKED_SOLO_5x5")
            .map(Record::from_queue)
    }
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

fn handle_ranked_stats(data: &serde_json::Value, state: &SharedState) -> Option<String> {
    let mut stats = serde_json::from_value::<RankedStats>(data.clone()).ok()?;
    let current = stats.solo_queue_record()?;
    stats.session = Some(lock_state(state).session(current));
    serde_json::to_string(&stats).ok()
}

pub fn spawn_lcu_listener(outbox: Outbox) {
    let state = Arc::new(Mutex::new(SessionTracker::default()));

    tokio::spawn(async move {
        tracing::info!("LCU Listener task spawned. Searching for active League client...");

        loop {
            run_irelia_cycle(&outbox, &state).await;
            tokio::time::sleep(RETRY_DELAY).await;
        }
    });
}

async fn run_irelia_cycle(outbox: &Outbox, state: &SharedState) {
    let Some(mut source) = IreliaSource::connect() else {
        return;
    };
    run_listener_cycle(&mut source, outbox, state).await;
}

async fn run_listener_cycle(source: &mut impl LcuSource, outbox: &Outbox, state: &SharedState) {
    let Some(summoner) = source.fetch(SUMMONER_URI).await else {
        return;
    };

    update_account(state, &summoner);

    let outbox_inner = outbox.clone();
    let state_inner = Arc::clone(state);
    let connection = source.subscribe(move |event| {
        handle_lcu_event(event, &outbox_inner, &state_inner);
    });

    if connection.is_none() {
        return;
    }

    tracing::info!(target: "lcu_monitor", "Connected to LCU WebSocket.");

    send_initial_sync(source, outbox, state).await;
    wait_until_disconnected(source).await;

    tracing::warn!(target: "lcu_monitor", "LCU connection lost. Retrying...");
}

fn update_account(state: &SharedState, summoner: &serde_json::Value) {
    let new_id = SummonerId::from_summoner(summoner);
    if !lock_state(state).switch_account(new_id) {
        return;
    }
    tracing::info!(target: "lcu_monitor", "Account detected: ID {:?}", new_id);
}

fn handle_lcu_event(payload: &EventData, outbox: &Outbox, state: &SharedState) {
    match payload.uri.as_str() {
        RANKED_STATS_URI => push_ranked_stats(&payload.data, outbox, state),
        SUMMONER_URI => push_raw(
            &payload.data,
            outbox,
            Topic::Summoner,
            "Pushing Summoner profile update",
        ),
        MATCH_HISTORY_URI => push_raw(
            &payload.data,
            outbox,
            Topic::MatchHistory,
            "Pushing Match History update",
        ),
        _ => {}
    }
}

fn push_ranked_stats(data: &serde_json::Value, outbox: &Outbox, state: &SharedState) {
    let Some(json) = handle_ranked_stats(data, state) else {
        return;
    };
    outbox.publish(Topic::RankedStats, json);
    tracing::info!(target: "overlay_update", "Pushing Ranked Stats update");
}

fn push_raw(data: &serde_json::Value, outbox: &Outbox, topic: Topic, log_message: &str) {
    outbox.publish(topic, data.to_string());
    tracing::info!(target: "overlay_update", "{}", log_message);
}

async fn send_initial_sync(source: &impl LcuSource, outbox: &Outbox, state: &SharedState) {
    sync_summoner(source, outbox).await;
    sync_ranked_stats(source, outbox, state).await;
    sync_recent_matches(source, outbox).await;
}

async fn sync_summoner(source: &impl LcuSource, outbox: &Outbox) {
    let Some(profile) = source.fetch(SUMMONER_URI).await else {
        return;
    };
    push_raw(
        &profile,
        outbox,
        Topic::Summoner,
        "Initial Sync: Summoner Profile sent",
    );
}

async fn sync_ranked_stats(source: &impl LcuSource, outbox: &Outbox, state: &SharedState) {
    let Some(stats) = source.fetch(RANKED_STATS_URI).await else {
        return;
    };
    let Some(json) = handle_ranked_stats(&stats, state) else {
        return;
    };
    outbox.publish(Topic::RankedStats, json);
    tracing::info!(target: "overlay_update", "Initial Sync: Ranked Stats sent");
}

async fn sync_recent_matches(source: &impl LcuSource, outbox: &Outbox) {
    let Some(matches) = source.fetch(MATCH_HISTORY_URI).await else {
        return;
    };
    push_raw(
        &matches,
        outbox,
        Topic::MatchHistory,
        "Pushing Match History update",
    );
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
    const RANKED_STATS_NO_RECORD: &str =
        include_str!("../tests/fixtures/ranked_stats_no_record.json");

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
        let cycle = run_listener_cycle(source, outbox, state);
        tokio::time::timeout(Duration::from_secs(600), cycle)
            .await
            .expect("cycle ends after disconnect");
        drain(&mut rx)
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
        assert_eq!(messages[0], fixture(SUMMONER));
        assert_eq!(messages[1]["queues"], fixture(RANKED_STATS)["queues"]);
        assert_eq!(messages[1]["session"], json!({"wins": 0, "losses": 0}));
        assert_eq!(messages[2], fixture(MATCHES));
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

        assert_eq!(messages[1]["session"], json!({"wins": 2, "losses": 1}));
    }

    #[tokio::test(start_paused = true)]
    async fn account_switch_resets_baseline() {
        let state = new_state();
        let mut first = MockSource::running(summoner_with_id(1), ranked_with_record(10, 5), 3);
        let mut second = MockSource::running(summoner_with_id(2), ranked_with_record(12, 6), 3);

        run_cycle(&mut first, &state).await;
        let messages = run_cycle(&mut second, &state).await;

        assert_eq!(messages[1]["session"], json!({"wins": 0, "losses": 0}));
    }

    fn ranked_json(text: &str, state: &SharedState) -> Option<Value> {
        let json = handle_ranked_stats(&fixture(text), state)?;
        Some(serde_json::from_str(&json).expect("ranked stats output is valid JSON"))
    }

    #[test]
    fn first_ranked_payload_sets_baseline() {
        let stats = ranked_json(RANKED_STATS, &new_state()).expect("solo queue present");

        assert_eq!(stats["session"], json!({"wins": 0, "losses": 0}));
        assert_eq!(stats["queues"], fixture(RANKED_STATS)["queues"]);
    }

    #[test]
    fn later_ranked_payload_reports_deltas() {
        let state = new_state();

        ranked_json(RANKED_STATS, &state);
        let stats = ranked_json(RANKED_STATS_AFTER_GAMES, &state).expect("solo queue present");

        assert_eq!(stats["session"], json!({"wins": 2, "losses": 1}));
    }

    #[test]
    fn ranked_payload_without_solo_queue_is_ignored() {
        assert_eq!(ranked_json(RANKED_STATS_FLEX_ONLY, &new_state()), None);
    }

    #[test]
    fn missing_wins_and_losses_count_as_zero() {
        let state = new_state();

        ranked_json(RANKED_STATS_NO_RECORD, &state);
        let stats = ranked_json(RANKED_STATS, &state).expect("solo queue present");

        assert_eq!(stats["session"], json!({"wins": 34, "losses": 29}));
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
}

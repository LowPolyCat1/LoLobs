use irelia::ws::{
    LcuWebSocket,
    types::{EventData, EventKind},
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::broadcast;

type ConcreteLcuClient = irelia::rest::LcuClient<irelia::requests::RequestClientType>;

const SUMMONER_URI: &str = "/lol-summoner/v1/current-summoner";
const RANKED_STATS_URI: &str = "/lol-ranked/v1/current-ranked-stats";
const MATCH_HISTORY_URI: &str = "/lol-match-history/v1/products/lol/current-summoner/matches";
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub wins: i64,
    pub losses: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RankedStats {
    pub queues: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
}

struct LcuState {
    summoner_id: Option<u64>,
    baseline: Option<(i64, i64)>,
}

type SharedState = Arc<Mutex<LcuState>>;

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

pub async fn fetch_endpoint(uri: &str) -> Option<serde_json::Value> {
    IreliaSource::connect()?.fetch(uri).await
}

fn lock_state(state: &SharedState) -> MutexGuard<'_, LcuState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn handle_ranked_stats(data: &serde_json::Value, state: &SharedState) -> Option<String> {
    let mut stats = serde_json::from_value::<RankedStats>(data.clone()).ok()?;

    let solo_q = stats
        .queues
        .iter()
        .find(|q| q["queueType"] == "RANKED_SOLO_5x5")?;

    let cur_w = solo_q["wins"].as_i64().unwrap_or(0);
    let cur_l = solo_q["losses"].as_i64().unwrap_or(0);

    let mut s = lock_state(state);
    let (base_w, base_l) = *s.baseline.get_or_insert_with(|| {
        tracing::info!(target: "lcu_monitor", "New baseline established: {}W - {}L", cur_w, cur_l);
        (cur_w, cur_l)
    });

    stats.session = Some(Session {
        wins: cur_w - base_w,
        losses: cur_l - base_l,
    });

    serde_json::to_string(&stats).ok()
}

pub fn spawn_lcu_listener(tx: broadcast::Sender<String>) {
    let state = Arc::new(Mutex::new(LcuState {
        summoner_id: None,
        baseline: None,
    }));

    tokio::spawn(async move {
        tracing::info!("LCU Listener task spawned. Searching for active League client...");

        loop {
            run_irelia_cycle(&tx, &state).await;
            tokio::time::sleep(RETRY_DELAY).await;
        }
    });
}

async fn run_irelia_cycle(tx: &broadcast::Sender<String>, state: &SharedState) {
    let Some(mut source) = IreliaSource::connect() else {
        return;
    };
    run_listener_cycle(&mut source, tx, state).await;
}

async fn run_listener_cycle(
    source: &mut impl LcuSource,
    tx: &broadcast::Sender<String>,
    state: &SharedState,
) {
    let Some(summoner) = source.fetch(SUMMONER_URI).await else {
        return;
    };

    update_account(state, &summoner);

    let tx_inner = tx.clone();
    let state_inner = Arc::clone(state);
    let connection = source.subscribe(move |event| {
        handle_lcu_event(event, &tx_inner, &state_inner);
    });

    if connection.is_none() {
        return;
    }

    tracing::info!(target: "lcu_monitor", "Connected to LCU WebSocket.");

    send_initial_sync(source, tx, state).await;
    wait_until_disconnected(source).await;

    tracing::warn!(target: "lcu_monitor", "LCU connection lost. Retrying...");
}

fn update_account(state: &SharedState, summoner: &serde_json::Value) {
    let new_id = summoner["summonerId"].as_u64();
    let mut s = lock_state(state);
    if s.summoner_id == new_id {
        return;
    }

    tracing::info!(target: "lcu_monitor", "Account detected: ID {:?}", new_id);
    s.summoner_id = new_id;
    s.baseline = None;
}

fn handle_lcu_event(payload: &EventData, tx: &broadcast::Sender<String>, state: &SharedState) {
    match payload.uri.as_str() {
        RANKED_STATS_URI => push_ranked_stats(&payload.data, tx, state),
        SUMMONER_URI => push_raw(&payload.data, tx, "Pushing Summoner profile update"),
        MATCH_HISTORY_URI => push_raw(&payload.data, tx, "Pushing Match History update"),
        _ => {}
    }
}

fn push_ranked_stats(
    data: &serde_json::Value,
    tx: &broadcast::Sender<String>,
    state: &SharedState,
) {
    let Some(json) = handle_ranked_stats(data, state) else {
        return;
    };
    let _ = tx.send(json);
    tracing::info!(target: "overlay_update", "Pushing Ranked Stats update");
}

fn push_raw(data: &serde_json::Value, tx: &broadcast::Sender<String>, log_message: &str) {
    let _ = tx.send(data.to_string());
    tracing::info!(target: "overlay_update", "{}", log_message);
}

async fn send_initial_sync(
    source: &impl LcuSource,
    tx: &broadcast::Sender<String>,
    state: &SharedState,
) {
    sync_summoner(source, tx).await;
    sync_ranked_stats(source, tx, state).await;
    sync_recent_matches(source, tx).await;
}

async fn sync_summoner(source: &impl LcuSource, tx: &broadcast::Sender<String>) {
    let Some(profile) = source.fetch(SUMMONER_URI).await else {
        return;
    };
    let Ok(json) = serde_json::to_string(&profile) else {
        return;
    };
    let _ = tx.send(json);
    tracing::info!(target: "overlay_update", "Initial Sync: Summoner Profile sent");
}

async fn sync_ranked_stats(
    source: &impl LcuSource,
    tx: &broadcast::Sender<String>,
    state: &SharedState,
) {
    let Some(stats) = source.fetch(RANKED_STATS_URI).await else {
        return;
    };
    let Some(json) = handle_ranked_stats(&stats, state) else {
        return;
    };
    let _ = tx.send(json);
    tracing::info!(target: "overlay_update", "Initial Sync: Ranked Stats sent");
}

async fn sync_recent_matches(source: &impl LcuSource, tx: &broadcast::Sender<String>) {
    let Some(matches) = source.fetch(MATCH_HISTORY_URI).await else {
        return;
    };
    let Ok(json) = serde_json::to_string(&matches) else {
        return;
    };
    let _ = tx.send(json);
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
        Arc::new(Mutex::new(LcuState {
            summoner_id: None,
            baseline: None,
        }))
    }

    fn drain(rx: &mut broadcast::Receiver<String>) -> Vec<Value> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .map(|msg| serde_json::from_str(&msg).expect("broadcast is valid JSON"))
            .collect()
    }

    async fn run_cycle(source: &mut MockSource, state: &SharedState) -> Vec<Value> {
        let (tx, mut rx) = broadcast::channel(32);
        let cycle = run_listener_cycle(source, &tx, state);
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
}

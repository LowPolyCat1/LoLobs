use irelia::ws::{
    LcuWebSocket,
    types::{EventData, EventKind},
};
use serde::{Deserialize, Serialize};
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

pub fn get_lcu_client() -> Option<ConcreteLcuClient> {
    let client = irelia::requests::new();
    irelia::rest::LcuClient::connect_with_request_client(&client).ok()
}

pub async fn fetch_endpoint(uri: &str) -> Option<serde_json::Value> {
    let client = get_lcu_client()?;
    fetch_with_client(&client, uri).await
}

async fn fetch_with_client(client: &ConcreteLcuClient, uri: &str) -> Option<serde_json::Value> {
    client.get::<serde_json::Value>(uri).await.ok()
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
            run_listener_cycle(&tx, &state).await;
            tokio::time::sleep(RETRY_DELAY).await;
        }
    });
}

async fn run_listener_cycle(tx: &broadcast::Sender<String>, state: &SharedState) {
    let Some((client, summoner)) = connect_and_fetch_summoner().await else {
        return;
    };

    update_account(state, &summoner);

    let mut ws_client = LcuWebSocket::new();
    let tx_inner = tx.clone();
    let state_inner = Arc::clone(state);
    let connection = ws_client.subscribe_closure(EventKind::json_api_event(), move |event| {
        handle_lcu_event(&event.2, &tx_inner, &state_inner);
    });

    if connection.is_none() {
        return;
    }

    tracing::info!(target: "lcu_monitor", "Connected to LCU WebSocket.");

    send_initial_sync(&client, tx, state).await;
    wait_until_disconnected(&client).await;

    tracing::warn!(target: "lcu_monitor", "LCU connection lost. Retrying...");
}

async fn connect_and_fetch_summoner() -> Option<(ConcreteLcuClient, serde_json::Value)> {
    let client = get_lcu_client()?;
    let summoner = fetch_with_client(&client, SUMMONER_URI).await?;
    Some((client, summoner))
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
    client: &ConcreteLcuClient,
    tx: &broadcast::Sender<String>,
    state: &SharedState,
) {
    sync_summoner(client, tx).await;
    sync_ranked_stats(client, tx, state).await;
    sync_recent_matches(client, tx).await;
}

async fn sync_summoner(client: &ConcreteLcuClient, tx: &broadcast::Sender<String>) {
    let Some(profile) = fetch_with_client(client, SUMMONER_URI).await else {
        return;
    };
    let Ok(json) = serde_json::to_string(&profile) else {
        return;
    };
    let _ = tx.send(json);
    tracing::info!(target: "overlay_update", "Initial Sync: Summoner Profile sent");
}

async fn sync_ranked_stats(
    client: &ConcreteLcuClient,
    tx: &broadcast::Sender<String>,
    state: &SharedState,
) {
    let Some(stats) = fetch_with_client(client, RANKED_STATS_URI).await else {
        return;
    };
    let Some(json) = handle_ranked_stats(&stats, state) else {
        return;
    };
    let _ = tx.send(json);
    tracing::info!(target: "overlay_update", "Initial Sync: Ranked Stats sent");
}

async fn sync_recent_matches(client: &ConcreteLcuClient, tx: &broadcast::Sender<String>) {
    let Some(matches) = fetch_recent_matches(client).await else {
        return;
    };
    let Ok(json) = serde_json::to_string(&matches) else {
        return;
    };
    let _ = tx.send(json);
}

async fn wait_until_disconnected(client: &ConcreteLcuClient) {
    while fetch_with_client(client, SUMMONER_URI).await.is_some() {
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

pub async fn fetch_recent_matches(client: &ConcreteLcuClient) -> Option<serde_json::Value> {
    fetch_with_client(client, MATCH_HISTORY_URI).await
}

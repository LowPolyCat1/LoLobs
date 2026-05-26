use irelia::ws::{LcuWebSocket, types::EventKind};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

type ConcreteLcuClient = irelia::rest::LcuClient<irelia::requests::RequestClientType>;

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

pub async fn get_lcu_client() -> Option<ConcreteLcuClient> {
    let client = irelia::requests::new();
    irelia::rest::LcuClient::connect_with_request_client(&client).ok()
}

pub async fn fetch_endpoint(uri: &str) -> Option<serde_json::Value> {
    let client: ConcreteLcuClient = get_lcu_client().await?;
    client.get::<serde_json::Value>(uri).await.ok()
}

fn route_event(
    uri: &str,
    data: &serde_json::Value,
    state: &Arc<Mutex<LcuState>>,
) -> Option<String> {
    match uri {
        "/lol-ranked/v1/current-ranked-stats" => handle_ranked_stats(data, state),
        "/lol-summoner/v1/current-summoner" => Some(data.to_string()),
        "/lol-match-history/v1/products/lol/current-summoner/matches" => Some(data.to_string()),
        _ => None,
    }
}

fn handle_ranked_stats(data: &serde_json::Value, state: &Arc<Mutex<LcuState>>) -> Option<String> {
    let mut stats = serde_json::from_value::<RankedStats>(data.clone()).ok()?;

    let solo_q = stats
        .queues
        .iter()
        .find(|q| q["queueType"] == "RANKED_SOLO_5x5")?;

    let cur_w = solo_q["wins"].as_i64().unwrap_or(0);
    let cur_l = solo_q["losses"].as_i64().unwrap_or(0);

    let mut s = state.lock().unwrap();
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
            let Some(summoner) = fetch_endpoint("/lol-summoner/v1/current-summoner").await else {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            };

            let new_id = summoner["summonerId"].as_u64();
            {
                let mut s = state.lock().unwrap();
                if s.summoner_id != new_id {
                    tracing::info!(target: "lcu_monitor", "Account detected: ID {:?}", new_id);
                    s.summoner_id = new_id;
                    s.baseline = None;
                }
            }

            let mut ws_client = LcuWebSocket::new();
            let tx_inner = tx.clone();
            let state_inner = Arc::clone(&state);

            let connection = ws_client.subscribe_closure(EventKind::json_api_event(), move |event| {
                let payload = &event.2;
                if let Some(json) = route_event(&payload.uri, &payload.data, &state_inner) {
                    let _ = tx_inner.send(json);
                    tracing::info!(target: "overlay_update", "Pushing update for {}", payload.uri);
                }
            });

            if connection.is_some() {
                tracing::info!(target: "lcu_monitor", "Connected to LCU WebSocket.");

                if let Some(profile) = fetch_endpoint("/lol-summoner/v1/current-summoner").await
                    && let Ok(json) = serde_json::to_string(&profile)
                {
                    let _ = tx.send(json);
                    tracing::info!(target: "overlay_update", "Initial Sync: Summoner Profile sent");
                }

                if let Some(stats) = fetch_endpoint("/lol-ranked/v1/current-ranked-stats").await
                    && let Some(json) = handle_ranked_stats(&stats, &state)
                {
                    let _ = tx.send(json);
                    tracing::info!(target: "overlay_update", "Initial Sync: Ranked Stats sent");
                }

                if let Some(matches) = fetch_recent_matches().await
                    && let Ok(json) = serde_json::to_string(&matches)
                {
                    let _ = tx.send(json);
                }

                while fetch_endpoint("/lol-summoner/v1/current-summoner")
                    .await
                    .is_some()
                {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }

                tracing::warn!(target: "lcu_monitor", "LCU connection lost. Retrying...");
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

pub async fn fetch_recent_matches() -> Option<serde_json::Value> {
    fetch_endpoint("/lol-match-history/v1/products/lol/current-summoner/matches").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fresh_state() -> Arc<Mutex<LcuState>> {
        Arc::new(Mutex::new(LcuState {
            summoner_id: None,
            baseline: None,
        }))
    }

    fn ranked_payload(wins: i64, losses: i64) -> serde_json::Value {
        json!({
            "queues": [
                { "queueType": "RANKED_SOLO_5x5", "wins": wins, "losses": losses },
                { "queueType": "RANKED_FLEX_SR",  "wins": 99,   "losses": 99 }
            ]
        })
    }

    #[test]
    fn first_call_establishes_baseline_with_zero_session() {
        let state = fresh_state();
        let out = handle_ranked_stats(&ranked_payload(10, 5), &state)
            .expect("ranked stats with solo queue should produce output");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(parsed["session"]["wins"], 0);
        assert_eq!(parsed["session"]["losses"], 0);
        assert_eq!(state.lock().unwrap().baseline, Some((10, 5)));
    }

    #[test]
    fn subsequent_call_returns_delta_against_baseline() {
        let state = fresh_state();
        let _ = handle_ranked_stats(&ranked_payload(10, 5), &state);
        let out = handle_ranked_stats(&ranked_payload(13, 7), &state)
            .expect("second call should produce output");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(parsed["session"]["wins"], 3);
        assert_eq!(parsed["session"]["losses"], 2);
    }

    #[test]
    fn baseline_is_only_set_once_across_calls() {
        let state = fresh_state();
        let _ = handle_ranked_stats(&ranked_payload(10, 5), &state);
        let _ = handle_ranked_stats(&ranked_payload(13, 7), &state);
        let _ = handle_ranked_stats(&ranked_payload(20, 10), &state);

        assert_eq!(state.lock().unwrap().baseline, Some((10, 5)));
    }

    #[test]
    fn missing_solo_queue_returns_none_and_leaves_baseline_unset() {
        let state = fresh_state();
        let payload = json!({
            "queues": [
                { "queueType": "RANKED_FLEX_SR", "wins": 5, "losses": 3 }
            ]
        });

        assert!(handle_ranked_stats(&payload, &state).is_none());
        assert_eq!(state.lock().unwrap().baseline, None);
    }

    #[test]
    fn malformed_payload_returns_none_and_leaves_baseline_unset() {
        let state = fresh_state();
        let payload = json!({ "foo": "bar" });

        assert!(handle_ranked_stats(&payload, &state).is_none());
        assert_eq!(state.lock().unwrap().baseline, None);
    }

    #[test]
    fn route_event_ranked_uri_returns_stats_with_session() {
        let state = fresh_state();
        let out = route_event(
            "/lol-ranked/v1/current-ranked-stats",
            &ranked_payload(10, 5),
            &state,
        )
        .expect("ranked uri with valid payload should route");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert!(parsed["session"].is_object());
    }

    #[test]
    fn route_event_summoner_uri_passes_data_through_as_string() {
        let state = fresh_state();
        let payload = json!({ "displayName": "Foo", "summonerId": 42 });
        let out = route_event("/lol-summoner/v1/current-summoner", &payload, &state)
            .expect("summoner uri should route");

        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), payload);
    }

    #[test]
    fn route_event_match_history_uri_passes_data_through_as_string() {
        let state = fresh_state();
        let payload = json!({ "games": { "games": [] } });
        let out = route_event(
            "/lol-match-history/v1/products/lol/current-summoner/matches",
            &payload,
            &state,
        )
        .expect("match-history uri should route");

        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), payload);
    }

    #[test]
    fn route_event_unknown_uri_returns_none() {
        let state = fresh_state();
        let out = route_event("/lol-unknown/v1/whatever", &json!({}), &state);

        assert!(out.is_none());
    }

    #[test]
    fn route_event_ranked_uri_with_malformed_payload_returns_none() {
        let state = fresh_state();
        let out = route_event(
            "/lol-ranked/v1/current-ranked-stats",
            &json!({ "foo": "bar" }),
            &state,
        );

        assert!(out.is_none());
        assert_eq!(state.lock().unwrap().baseline, None);
    }
}

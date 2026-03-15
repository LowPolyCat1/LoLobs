use irelia::ws::{LcuWebSocket, types::EventKind};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

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

pub async fn get_initial_rank() -> Option<String> {
    let client = irelia::requests::new();
    let lcu_client = irelia::rest::LcuClient::connect_with_request_client(&client).ok()?;
    let stats: serde_json::Value = lcu_client
        .get("/lol-ranked/v1/current-ranked-stats")
        .await
        .ok()?;

    let mut payload = stats.clone();
    payload["session"] = serde_json::json!({ "wins": 0, "losses": 0 });
    serde_json::to_string(&payload).ok()
}

pub fn spawn_lcu_listener(tx: Arc<broadcast::Sender<String>>) {
    let rt_handle = tokio::runtime::Handle::current();
    let session_baseline = Arc::new(Mutex::new(None::<(i64, i64)>));
    let last_payload = Arc::new(Mutex::new(None::<String>));

    std::thread::spawn(move || {
        loop {
            let mut ws_client = LcuWebSocket::new();
            let session_clone = Arc::clone(&session_baseline);
            let tx_clone = Arc::clone(&tx);
            let cache_clone = Arc::clone(&last_payload);

            let _connection =
                ws_client.subscribe_closure(EventKind::json_api_event(), move |event| {
                    let uri = &event.2.uri;

                    if uri == "/lol-ranked/v1/current-ranked-stats"
                        || uri == "/lol-summoner/v1/current-summoner"
                    {
                        println!(">>> LCU Event Triggered: {}", uri);

                        let tx_inner = Arc::clone(&tx_clone);
                        let session_inner = Arc::clone(&session_clone);
                        let cache_inner = Arc::clone(&cache_clone);

                        tokio::runtime::Handle::current().block_on(async move {
                            if let Some(data_str) = get_initial_rank().await {
                                if let Ok(mut stats) =
                                    serde_json::from_str::<RankedStats>(&data_str)
                                {
                                    let solo_q = stats
                                        .queues
                                        .iter()
                                        .find(|q| q["queueType"] == "RANKED_SOLO_5x5");

                                    if let Some(q) = solo_q {
                                        let cur_w = q["wins"].as_i64().unwrap_or(0);
                                        let cur_l = q["losses"].as_i64().unwrap_or(0);

                                        let mut baseline = session_inner.lock().unwrap();
                                        let (base_w, base_l) =
                                            *baseline.get_or_insert((cur_w, cur_l));

                                        stats.session = Some(Session {
                                            wins: cur_w - base_w,
                                            losses: cur_l - base_l,
                                        });

                                        if let Ok(json) = serde_json::to_string(&stats) {
                                            *cache_inner.lock().unwrap() = Some(json.clone());
                                            let _ = tx_inner.send(json);
                                            println!(">>> Automatic Update Sent to OBS.");
                                        }
                                    }
                                }
                            }
                        });
                    }
                });

            if _connection.is_some() {
                println!(">>> LCU Connected. Session baseline is preserved.");

                let tx_init = Arc::clone(&tx);
                let session_init = Arc::clone(&session_baseline);
                let cache_init = Arc::clone(&last_payload);

                rt_handle.block_on(async {
                    if let Some(data_str) = get_initial_rank().await {
                        if let Ok(mut stats) = serde_json::from_str::<RankedStats>(&data_str) {
                            let solo_q = stats
                                .queues
                                .iter()
                                .find(|q| q["queueType"] == "RANKED_SOLO_5x5");
                            if let Some(q) = solo_q {
                                let cur_w = q["wins"].as_i64().unwrap_or(0);
                                let cur_l = q["losses"].as_i64().unwrap_or(0);

                                let mut b = session_init.lock().unwrap();
                                let (base_w, base_l) = *b.get_or_insert((cur_w, cur_l));

                                stats.session = Some(Session {
                                    wins: cur_w - base_w,
                                    losses: cur_l - base_l,
                                });

                                if let Ok(json) = serde_json::to_string(&stats) {
                                    *cache_init.lock().unwrap() = Some(json.clone());
                                    let _ = tx_init.send(json);
                                }
                            }
                        }
                    }
                });

                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));

                    if let Some(cached) = last_payload.lock().unwrap().clone() {
                        let _ = tx.send(cached);
                    }

                    if rt_handle.block_on(async { crate::lcu::get_summoner_data().await.is_none() })
                    {
                        println!(">>> LCU Disconnected. (Baseline preserved)");
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

pub async fn get_summoner_data() -> Option<String> {
    let client = irelia::requests::new();
    let lcu_client = irelia::rest::LcuClient::connect_with_request_client(&client).ok()?;
    let summoner: serde_json::Value = lcu_client
        .get("/lol-summoner/v1/current-summoner")
        .await
        .ok()?;
    serde_json::to_string(&summoner).ok()
}

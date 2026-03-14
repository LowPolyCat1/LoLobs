use irelia::ws::{LcuWebSocket, types::EventKind};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub async fn get_initial_rank() -> Option<String> {
    let client = irelia::requests::new();
    let lcu_client = irelia::rest::LcuClient::connect_with_request_client(&client).ok()?;

    let stats: serde_json::Value = match lcu_client.get("/lol-ranked/v1/current-ranked-stats").await
    {
        Ok(s) => s,
        Err(_) => serde_json::json!({"queues": [], "errorMessage": "LCU_OFFLINE"}),
    };

    let final_payload = serde_json::json!({
        "queues": stats["queues"],
        "session": { "wins": 0, "losses": 0 }
    });

    serde_json::to_string(&final_payload).ok()
}
pub fn spawn_lcu_listener(tx: Arc<broadcast::Sender<String>>) {
    let rt_handle = tokio::runtime::Handle::current();

    std::thread::spawn(move || {
        let session_start = Arc::new(Mutex::new(None));

        loop {
            let mut ws_client = LcuWebSocket::new();
            let session_clone = Arc::clone(&session_start);
            let tx_clone = Arc::clone(&tx);

            let connection = ws_client.subscribe_closure(EventKind::json_api_event(), {
                let tx_internal = Arc::clone(&tx_clone);
                let session_internal = Arc::clone(&session_clone);
                move |event| {
                    if event.2.uri == "/lol-ranked/v1/current-ranked-stats" {
                        let data = &event.2.data;
                        let solo_q = data["queues"].as_array().and_then(|queues| {
                            queues.iter().find(|q| q["queueType"] == "RANKED_SOLO_5x5")
                        });

                        let (cur_w, cur_l) = match solo_q {
                            Some(q) => (
                                q["wins"].as_i64().unwrap_or(0),
                                q["losses"].as_i64().unwrap_or(0),
                            ),
                            None => (0, 0),
                        };

                        let mut start = session_internal.lock().unwrap();
                        let (base_w, base_l) = *start.get_or_insert((cur_w, cur_l));

                        let mut output = data.clone();
                        output["session"] = serde_json::json!({
                            "wins": cur_w - base_w,
                            "losses": cur_l - base_l
                        });

                        let _ =
                            tx_internal.send(serde_json::to_string(&output).unwrap_or_default());
                    }
                }
            });

            if connection.is_some() {
                println!("LCU Connected: Blasting initial data...");

                let tx_init = Arc::clone(&tx);
                rt_handle.spawn(async move {
                    if let Some(rank_data) = crate::lcu::get_initial_rank().await {
                        let _ = tx_init.send(rank_data);
                    }
                    if let Some(summoner_data) = crate::lcu::get_summoner_data().await {
                        let _ = tx_init.send(summoner_data);
                    }
                });

                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));

                    let is_disconnected = rt_handle
                        .block_on(async { crate::lcu::get_summoner_data().await.is_none() });

                    if is_disconnected {
                        println!("LCU Disconnected. Resetting...");
                        break;
                    }
                }
            }

            if let Ok(mut start) = session_start.lock() {
                *start = None;
            }

            std::thread::sleep(std::time::Duration::from_secs(10));
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

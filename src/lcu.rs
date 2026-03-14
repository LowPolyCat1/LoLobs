use irelia::ws::{LcuWebSocket, types::EventKind};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub async fn get_initial_rank() -> Option<String> {
    let client = irelia::requests::new();
    let lcu_client = irelia::rest::LcuClient::connect_with_request_client(&client).ok()?;

    let stats: serde_json::Value = lcu_client
        .get("/lol-ranked/v1/current-ranked-stats")
        .await
        .ok()?;

    let final_payload = serde_json::json!({
        "queues": stats["queues"],
        "session": {
            "wins": 0,
            "losses": 0
        }
    });

    serde_json::to_string(&final_payload).ok()
}

pub fn spawn_lcu_listener(tx: Arc<broadcast::Sender<String>>) {
    std::thread::spawn(move || {
        let mut ws_client = LcuWebSocket::new();
        let session_start = Arc::new(Mutex::new(None));

        let _ = ws_client.subscribe_closure(EventKind::json_api_event(), move |event| {
            if event.2.uri == "/lol-ranked/v1/current-ranked-stats" {
                let data = &event.2.data;

                if let Some(solo_q) = data["queues"]
                    .as_array()
                    .and_then(|queues| queues.iter().find(|q| q["queueType"] == "RANKED_SOLO_5x5"))
                {
                    let cur_w = solo_q["wins"].as_i64().unwrap_or(0);
                    let cur_l = solo_q["losses"].as_i64().unwrap_or(0);

                    let mut start = session_start.lock().unwrap();
                    let (base_w, base_l) = *start.get_or_insert((cur_w, cur_l));

                    let mut output = data.clone();
                    output["session"] = serde_json::json!({
                        "wins": cur_w - base_w,
                        "losses": cur_l - base_l
                    });

                    if let Ok(json_str) = serde_json::to_string(&output) {
                        let _ = tx.send(json_str);
                    }
                }
            }
        });

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
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

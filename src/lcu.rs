use irelia::ws::{LcuWebSocket, types::EventKind};
use std::sync::Arc;
use tokio::sync::broadcast;

pub async fn get_initial_rank() -> Option<String> {
    let client = irelia::requests::new();
    let lcu_client = irelia::rest::LcuClient::connect_with_request_client(&client).ok()?;

    let stats: serde_json::Value = lcu_client
        .get("/lol-ranked/v1/current-ranked-stats")
        .await
        .ok()?;

    serde_json::to_string(&stats).ok()
}

pub fn spawn_lcu_listener(tx: Arc<broadcast::Sender<String>>) {
    std::thread::spawn(move || {
        let mut ws_client = LcuWebSocket::new();
        let _ = ws_client.subscribe_closure(EventKind::json_api_event(), move |event| {
            if event.2.uri == "/lol-ranked/v1/current-ranked-stats" {
                if let Ok(json_str) = serde_json::to_string(&event.2.data) {
                    let _ = tx.send(json_str);
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

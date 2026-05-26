//! Live smoke tests. Require:
//!   - The League client (LCU) running and logged in
//!   - The LoLobs server running on 127.0.0.1:2009 (`cargo run`)
//!
//! Run with: `cargo test -- --ignored`

use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "ws://127.0.0.1:2009/ws";
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(15);

#[tokio::test]
#[ignore]
async fn ws_emits_valid_json_within_timeout() {
    let (mut ws, _) = connect_async(WS_URL)
        .await
        .expect("connect to ws://127.0.0.1:2009/ws — is the server running?");

    let frame = timeout(MESSAGE_TIMEOUT, ws.next())
        .await
        .expect("no message received within 15s — is the LoL client logged in?")
        .expect("ws stream ended without a message")
        .expect("ws read error");

    let Message::Text(text) = frame else {
        panic!("expected a text frame, got: {:?}", frame);
    };

    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("server emitted a non-JSON text frame");

    assert!(
        parsed.is_object() || parsed.is_array(),
        "expected JSON object or array, got: {}",
        parsed
    );
}

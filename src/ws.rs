use crate::lcu::fetch_endpoint;
use actix_web::{HttpRequest, HttpResponse, web};
use actix_ws::{Message, MessageStream, Session};
use tokio::sync::broadcast;

enum Flow {
    Continue,
    Stop,
}

pub fn start(
    req: &HttpRequest,
    stream: web::Payload,
    receiver: broadcast::Receiver<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let (response, session, messages) = actix_ws::handle(req, stream)?;

    actix_web::rt::spawn(send_initial_state(session.clone()));
    actix_web::rt::spawn(run_session(session, messages, receiver));

    Ok(response)
}

async fn run_session(
    session: Session,
    messages: MessageStream,
    receiver: broadcast::Receiver<String>,
) {
    tokio::select! {
        _ = forward_broadcasts(session.clone(), receiver) => {}
        _ = handle_incoming(session.clone(), messages) => {}
    }

    let _ = session.close(None).await;
}

async fn send_initial_state(mut session: Session) {
    let ranked = fetch_endpoint("/lol-ranked/v1/current-ranked-stats").await;
    let summoner = fetch_endpoint("/lol-summoner/v1/current-summoner").await;

    send_optional(&mut session, ranked).await;
    send_optional(&mut session, summoner).await;
}

async fn send_optional(session: &mut Session, value: Option<serde_json::Value>) {
    let Some(value) = value else {
        return;
    };
    let _ = session.text(value.to_string()).await;
}

// Known bug #10: a lagged receiver also ends the stream. Kept until that issue is fixed.
async fn forward_broadcasts(mut session: Session, mut receiver: broadcast::Receiver<String>) {
    while let Ok(msg) = receiver.recv().await
        && session.text(msg).await.is_ok()
    {}
}

async fn handle_incoming(mut session: Session, mut messages: MessageStream) {
    while let Some(Ok(msg)) = messages.recv().await
        && let Flow::Continue = handle_message(&mut session, msg).await
    {}
}

async fn handle_message(session: &mut Session, msg: Message) -> Flow {
    match msg {
        Message::Ping(bytes) => session
            .pong(&bytes)
            .await
            .map_or(Flow::Stop, |()| Flow::Continue),
        Message::Close(reason) => {
            let _ = session.clone().close(reason).await;
            Flow::Stop
        }
        _ => Flow::Continue,
    }
}

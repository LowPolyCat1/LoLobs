use crate::snapshot::SharedSnapshot;
use actix_web::{HttpRequest, HttpResponse, web};
use actix_ws::{Closed, Message, MessageStream, Session};
use tokio::sync::broadcast;

enum Flow {
    Continue,
    Stop,
}

pub fn start(
    req: &HttpRequest,
    stream: web::Payload,
    receiver: broadcast::Receiver<String>,
    snapshot: SharedSnapshot,
) -> Result<HttpResponse, actix_web::Error> {
    let (response, session, messages) = actix_ws::handle(req, stream)?;

    actix_web::rt::spawn(run_session(session, messages, receiver, snapshot));

    Ok(response)
}

async fn run_session(
    session: Session,
    messages: MessageStream,
    receiver: broadcast::Receiver<String>,
    snapshot: SharedSnapshot,
) {
    tokio::select! {
        _ = send_snapshot_then_forward(session.clone(), receiver, snapshot) => {}
        _ = handle_incoming(session.clone(), messages) => {}
    }

    let _ = session.close(None).await;
}

async fn send_snapshot_then_forward(
    mut session: Session,
    receiver: broadcast::Receiver<String>,
    snapshot: SharedSnapshot,
) {
    if send_all(&mut session, snapshot.messages()).await.is_err() {
        return;
    }
    forward_broadcasts(session, receiver).await;
}

async fn send_all(session: &mut Session, messages: Vec<String>) -> Result<(), Closed> {
    for msg in messages {
        session.text(msg).await?;
    }
    Ok(())
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

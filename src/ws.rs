use crate::snapshot::SharedSnapshot;
use actix_web::{HttpRequest, HttpResponse, web};
use actix_ws::{Closed, Message, MessageStream, Session};
use tokio::sync::broadcast::{self, error::RecvError};

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
    forward_broadcasts(session, receiver, snapshot).await;
}

async fn send_all(session: &mut Session, messages: Vec<String>) -> Result<(), Closed> {
    for msg in messages {
        session.text(msg).await?;
    }
    Ok(())
}

async fn forward_broadcasts(
    mut session: Session,
    mut receiver: broadcast::Receiver<String>,
    snapshot: SharedSnapshot,
) {
    while let Some(messages) = next_messages(receiver.recv().await, &snapshot)
        && send_all(&mut session, messages).await.is_ok()
    {}
}

fn next_messages(
    received: Result<String, RecvError>,
    snapshot: &SharedSnapshot,
) -> Option<Vec<String>> {
    match received {
        Ok(msg) => Some(vec![msg]),
        Err(RecvError::Lagged(skipped)) => {
            tracing::warn!(target: "ws", "Client skipped {} broadcast messages. Resending snapshot.", skipped);
            Some(snapshot.messages())
        }
        Err(RecvError::Closed) => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Outbox;
    use crate::snapshot::test_messages::{json, matches, publish, ranked, summoner};

    #[test]
    fn received_message_is_forwarded() {
        let outbox = Outbox::new(8);

        let messages = next_messages(Ok("ranked".to_string()), &outbox.snapshot());

        assert_eq!(messages, Some(vec!["ranked".to_string()]));
    }

    #[test]
    fn closed_channel_stops_forwarding() {
        let outbox = Outbox::new(8);

        assert_eq!(
            next_messages(Err(RecvError::Closed), &outbox.snapshot()),
            None
        );
    }

    #[tokio::test]
    async fn lagged_client_receives_snapshot() {
        let outbox = Outbox::new(1);
        let mut receiver = outbox.subscribe();
        publish(&outbox, &summoner("PlayerOne"));
        publish(&outbox, &ranked(10));
        publish(&outbox, &matches());

        let received = receiver.recv().await;
        let messages = next_messages(received, &outbox.snapshot());

        assert_eq!(
            messages,
            Some(vec![
                json(&summoner("PlayerOne")),
                json(&ranked(10)),
                json(&matches())
            ])
        );
    }
}

use crate::message::OverlayMessage;
use std::sync::{Arc, PoisonError, RwLock};
use tokio::sync::broadcast;

#[derive(Default)]
struct Snapshot {
    summoner: Option<String>,
    ranked: Option<String>,
    matches: Option<String>,
}

impl Snapshot {
    fn set(&mut self, message: &OverlayMessage, json: String) {
        *self.slot(message) = Some(json);
    }

    fn slot(&mut self, message: &OverlayMessage) -> &mut Option<String> {
        match message {
            OverlayMessage::Summoner(_) => &mut self.summoner,
            OverlayMessage::Ranked(_) => &mut self.ranked,
            OverlayMessage::Matches(_) => &mut self.matches,
        }
    }

    fn messages(&self) -> Vec<String> {
        [&self.summoner, &self.ranked, &self.matches]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }
}

#[derive(Clone, Default)]
pub struct SharedSnapshot(Arc<RwLock<Snapshot>>);

impl SharedSnapshot {
    pub fn messages(&self) -> Vec<String> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .messages()
    }

    fn set(&self, message: &OverlayMessage, json: String) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .set(message, json);
    }
}

#[derive(Clone)]
pub struct Outbox {
    tx: broadcast::Sender<String>,
    snapshot: SharedSnapshot,
}

impl Outbox {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            snapshot: SharedSnapshot::default(),
        }
    }

    // The snapshot is updated first: a client that subscribes between both steps
    // gets the message twice instead of missing it.
    pub fn publish(&self, message: &OverlayMessage) -> Result<(), serde_json::Error> {
        let json = serde_json::to_string(message)?;
        self.snapshot.set(message, json.clone());
        let _ = self.tx.send(json);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn snapshot(&self) -> SharedSnapshot {
        self.snapshot.clone()
    }
}

#[cfg(test)]
pub mod test_messages {
    use crate::message::{OverlayMessage, RankedSummary, Record, RiotId, Tier};

    pub fn summoner(name: &str) -> OverlayMessage {
        OverlayMessage::Summoner(RiotId {
            game_name: name.to_string(),
            tag_line: None,
        })
    }

    pub fn ranked(league_points: i64) -> OverlayMessage {
        OverlayMessage::Ranked(RankedSummary {
            tier: Tier("GOLD".to_string()),
            division: None,
            league_points,
            season: Record { wins: 0, losses: 0 },
            session: Record { wins: 0, losses: 0 },
            lp_change: None,
        })
    }

    pub fn matches() -> OverlayMessage {
        OverlayMessage::Matches(Vec::new())
    }

    pub fn json(message: &OverlayMessage) -> String {
        serde_json::to_string(message).expect("message serializes")
    }

    pub fn publish(outbox: &super::Outbox, message: &OverlayMessage) {
        outbox.publish(message).expect("message serializes");
    }
}

#[cfg(test)]
mod tests {
    use super::test_messages::{json, matches, publish, ranked, summoner};
    use super::*;

    #[test]
    fn new_client_receives_full_snapshot_in_order() {
        let outbox = Outbox::new(8);
        publish(&outbox, &matches());
        publish(&outbox, &ranked(10));
        publish(&outbox, &summoner("PlayerOne"));

        let messages = outbox.snapshot().messages();

        assert_eq!(
            messages,
            [
                json(&summoner("PlayerOne")),
                json(&ranked(10)),
                json(&matches())
            ]
        );
    }

    #[test]
    fn snapshot_skips_empty_topics() {
        let outbox = Outbox::new(8);
        publish(&outbox, &ranked(10));

        assert_eq!(outbox.snapshot().messages(), [json(&ranked(10))]);
    }

    #[test]
    fn snapshot_keeps_latest_message_per_topic() {
        let outbox = Outbox::new(8);
        publish(&outbox, &ranked(10));
        publish(&outbox, &ranked(20));

        assert_eq!(outbox.snapshot().messages(), [json(&ranked(20))]);
    }

    #[test]
    fn publish_broadcasts_to_subscribers() {
        let outbox = Outbox::new(8);
        let mut rx = outbox.subscribe();

        publish(&outbox, &summoner("PlayerOne"));

        assert_eq!(rx.try_recv().ok(), Some(json(&summoner("PlayerOne"))));
    }
}

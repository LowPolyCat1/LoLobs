use std::sync::{Arc, PoisonError, RwLock};
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug)]
pub enum Topic {
    Summoner,
    RankedStats,
    MatchHistory,
}

#[derive(Default)]
struct Snapshot {
    summoner: Option<String>,
    ranked_stats: Option<String>,
    match_history: Option<String>,
}

impl Snapshot {
    fn set(&mut self, topic: Topic, json: String) {
        *self.slot(topic) = Some(json);
    }

    fn slot(&mut self, topic: Topic) -> &mut Option<String> {
        match topic {
            Topic::Summoner => &mut self.summoner,
            Topic::RankedStats => &mut self.ranked_stats,
            Topic::MatchHistory => &mut self.match_history,
        }
    }

    fn messages(&self) -> Vec<String> {
        [&self.summoner, &self.ranked_stats, &self.match_history]
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

    fn set(&self, topic: Topic, json: String) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .set(topic, json);
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
    pub fn publish(&self, topic: Topic, json: String) {
        self.snapshot.set(topic, json.clone());
        let _ = self.tx.send(json);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn snapshot(&self) -> SharedSnapshot {
        self.snapshot.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_client_receives_full_snapshot_in_order() {
        let outbox = Outbox::new(8);
        outbox.publish(Topic::MatchHistory, "matches".to_string());
        outbox.publish(Topic::RankedStats, "ranked".to_string());
        outbox.publish(Topic::Summoner, "summoner".to_string());

        let messages = outbox.snapshot().messages();

        assert_eq!(messages, ["summoner", "ranked", "matches"]);
    }

    #[test]
    fn snapshot_skips_empty_topics() {
        let outbox = Outbox::new(8);
        outbox.publish(Topic::RankedStats, "ranked".to_string());

        assert_eq!(outbox.snapshot().messages(), ["ranked"]);
    }

    #[test]
    fn snapshot_keeps_latest_message_per_topic() {
        let outbox = Outbox::new(8);
        outbox.publish(Topic::RankedStats, "old".to_string());
        outbox.publish(Topic::RankedStats, "new".to_string());

        assert_eq!(outbox.snapshot().messages(), ["new"]);
    }

    #[test]
    fn publish_broadcasts_to_subscribers() {
        let outbox = Outbox::new(8);
        let mut rx = outbox.subscribe();

        outbox.publish(Topic::Summoner, "summoner".to_string());

        assert_eq!(rx.try_recv().ok().as_deref(), Some("summoner"));
    }
}

//! Coalesced read acks.
//!
//! While a channel is open and focused, every incoming message used to send
//! its own `POST /channels/{c}/messages/{m}/ack`: a busy channel meant one
//! request per message, a steady stream no person produces. The official
//! client waits a moment and acks only the newest message. This queue does
//! the same: [`AckQueue::push`] records the newest message per channel and
//! asks for a single flush timer; [`AckQueue::drain`] hands over one ack per
//! channel when it fires.

use std::collections::HashMap;
use std::time::Duration;

use crate::net::model::{ChannelId, MessageId};

/// How long incoming messages are collected before the ack goes out.
pub const ACK_DELAY: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub struct AckQueue {
    pending: HashMap<ChannelId, MessageId>,
    armed: bool,
}

impl AckQueue {
    /// Records that `message` in `channel` was read. Returns `true` when the
    /// caller must start the flush timer (none is running yet).
    pub fn push(&mut self, channel: ChannelId, message: MessageId) -> bool {
        self.pending
            .entry(channel)
            .and_modify(|m| *m = (*m).max(message))
            .or_insert(message);
        !std::mem::replace(&mut self.armed, true)
    }

    /// The channel was acked directly (opened, marked read): nothing older
    /// needs to go out for it.
    pub fn forget(&mut self, channel: ChannelId) {
        self.pending.remove(&channel);
    }

    /// The flush timer fired: one ack per channel, the newest message.
    pub fn drain(&mut self) -> Vec<(ChannelId, MessageId)> {
        self.armed = false;
        let mut out: Vec<_> = self.pending.drain().collect();
        out.sort_unstable();
        out
    }

    /// Logout / account change: drop everything.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::model::Snowflake;

    #[test]
    fn a_burst_becomes_one_ack_per_channel() {
        let mut q = AckQueue::default();
        let (a, b) = (Snowflake(1), Snowflake(2));
        assert!(q.push(a, Snowflake(10)));
        assert!(!q.push(a, Snowflake(12)));
        assert!(!q.push(a, Snowflake(11)));
        assert!(!q.push(b, Snowflake(5)));
        assert_eq!(q.drain(), vec![(a, Snowflake(12)), (b, Snowflake(5))]);
        assert!(q.drain().is_empty());
        // The next message arms a new timer.
        assert!(q.push(a, Snowflake(13)));
    }

    #[test]
    fn forget_and_clear() {
        let mut q = AckQueue::default();
        q.push(Snowflake(1), Snowflake(1));
        q.push(Snowflake(2), Snowflake(2));
        q.forget(Snowflake(1));
        assert_eq!(q.drain(), vec![(Snowflake(2), Snowflake(2))]);
        q.push(Snowflake(3), Snowflake(3));
        q.clear();
        assert!(q.drain().is_empty());
        assert!(q.push(Snowflake(3), Snowflake(4)));
    }
}

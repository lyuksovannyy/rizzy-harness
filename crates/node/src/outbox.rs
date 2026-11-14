//! What the node sends the master, in two lanes (ADOPTION_PLAN A.21).
//!
//! **In order:** events, snapshots, `deleted`, artifact chunks and the acks
//! of every command that runs in a session's lane. They go in the order
//! they were queued: a session's events in `seq` order (each is queued
//! under the session's `head`), and a create's ack after its
//! `session_created` (`Master::create_session` relies on it). At most
//! `OUT_QUEUE` messages and `OUT_BYTES` of them wait; past that the sender
//! waits for room ([`Outbox::send`]: the back-pressure that keeps a session
//! from running further ahead of what the person sees than the queue
//! holds), or the message is dropped ([`Outbox::try_send`], for what is
//! asked for again).
//!
//! **Ahead:** the acks of the commands the node's command loop answers
//! inline (cancel, approval answer, `stop_background`). No order between
//! them and the output is relied on (an approval request is an event the
//! master has before anyone can answer it), so they pass whatever output is
//! queued, and they never wait: at most `URGENT_QUEUE` are kept, and past
//! that one is dropped (its command was acted on; the master's request
//! times out). While output waits, at most `URGENT_BURST` of them go in a
//! row, so a master that floods inline commands cannot starve the output.
//!
//! A feeder task moves one message at a time into the link's channel (one
//! slot), choosing when the slot frees: ahead of a new ack there is then at
//! most the message in that slot and the frame the link is writing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use harness_protocol::NodeToMaster;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc};

/// Messages in order waiting for the link.
pub(crate) const OUT_QUEUE: usize = 256;
/// ...and at most this many bytes of them, as sent. A larger message still
/// goes, alone, once the queue is empty. About 128 events of `bash` output
/// (8.1 KiB each, B.7 re-check B7-D3): 4 s at 256 KiB/s.
pub(crate) const OUT_BYTES: usize = 1 << 20;
/// Acks of inline commands kept while the link takes nothing: as many as
/// the node's in queue holds commands.
pub(crate) const URGENT_QUEUE: usize = crate::IN_QUEUE;
/// Of those acks, at most this many in a row while output waits.
pub(crate) const URGENT_BURST: usize = 8;

struct Queued {
    msg: NodeToMaster,
    /// Its room in the queue (a slot and its bytes), given back when it
    /// leaves the queue.
    _room: (OwnedSemaphorePermit, OwnedSemaphorePermit),
}

#[derive(Default)]
struct Lanes {
    ordered: VecDeque<Queued>,
    urgent: VecDeque<NodeToMaster>,
    /// Urgent messages sent in a row while ordered ones waited.
    run: usize,
    /// A newer link took over: send what is queued, then end.
    retired: bool,
    closed: bool,
}

/// What waits for the master (diagnostics and tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Backlog {
    /// Messages in order (events and the rest).
    pub queued: usize,
    /// Their bytes, as sent.
    pub bytes: usize,
    /// Acks of inline commands.
    pub urgent: usize,
    /// Senders waiting for room (a session's append among them).
    pub waiting: usize,
}

pub(crate) struct Outbox {
    lanes: Mutex<Lanes>,
    ready: Notify,
    slots: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
    waiting: AtomicUsize,
    /// The link's channel, to tell when it is empty (`flush`). The feeder
    /// holds its only strong sender: when the feeder ends, the link's
    /// receiver sees the end once it has taken what is in the channel.
    link: mpsc::WeakSender<NodeToMaster>,
}

/// The size of `msg` as the link sends it.
fn wire_len(msg: &NodeToMaster) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut c = Count(0);
    let _ = serde_json::to_writer(&mut c, msg);
    c.0
}

/// Room for `size` bytes: at least one permit, at most the whole queue.
fn permits(size: usize) -> u32 {
    size.clamp(1, OUT_BYTES) as u32
}

struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Outbox {
    /// A new outbox and the link's end of it. Its feeder runs until the
    /// receiver is dropped, [`Outbox::close`], or [`Outbox::retire`] and an
    /// empty queue.
    pub(crate) fn open() -> (Arc<Self>, mpsc::Receiver<NodeToMaster>) {
        let (tx, rx) = mpsc::channel(1);
        let this = Arc::new(Self {
            lanes: Mutex::new(Lanes::default()),
            ready: Notify::new(),
            slots: Arc::new(Semaphore::new(OUT_QUEUE)),
            bytes: Arc::new(Semaphore::new(OUT_BYTES)),
            waiting: AtomicUsize::new(0),
            link: tx.downgrade(),
        });
        tokio::spawn(this.clone().feed(tx));
        (this, rx)
    }

    fn lanes(&self) -> MutexGuard<'_, Lanes> {
        self.lanes.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.lanes().closed
    }

    /// Drop everything queued and end the feeder; senders waiting for room
    /// return. What they carried is in the store (events) or asked for
    /// again (the rest); the next hello replays the events.
    fn close(&self) {
        let dropped = {
            let mut l = self.lanes();
            l.closed = true;
            (
                std::mem::take(&mut l.ordered),
                std::mem::take(&mut l.urgent),
            )
        };
        drop(dropped);
        self.slots.close();
        self.bytes.close();
        self.ready.notify_one();
    }

    /// A newer link took over: what is queued still goes to this one (as
    /// with one channel, whose receiver read on until it was empty), then
    /// it ends.
    pub(crate) fn retire(&self) {
        self.lanes().retired = true;
        self.ready.notify_one();
    }

    fn push(&self, q: Queued) {
        let mut l = self.lanes();
        if !l.closed {
            l.ordered.push_back(q);
            drop(l);
            self.ready.notify_one();
        }
    }

    /// Queue `msg` in order, waiting for room.
    pub(crate) async fn send(&self, msg: NodeToMaster) {
        let n = permits(wire_len(&msg));
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let _w = Waiting(&self.waiting);
        let Ok(slot) = self.slots.clone().acquire_owned().await else {
            return;
        };
        let Ok(bytes) = self.bytes.clone().acquire_many_owned(n).await else {
            return;
        };
        self.push(Queued {
            msg,
            _room: (slot, bytes),
        });
    }

    /// Queue `msg` in order if there is room now. False: dropped.
    pub(crate) fn try_send(&self, msg: NodeToMaster) -> bool {
        let n = permits(wire_len(&msg));
        let Ok(slot) = self.slots.clone().try_acquire_owned() else {
            return false;
        };
        let Ok(bytes) = self.bytes.clone().try_acquire_many_owned(n) else {
            return false;
        };
        self.push(Queued {
            msg,
            _room: (slot, bytes),
        });
        true
    }

    /// Queue `msg` ahead of the output, never waiting. False: dropped
    /// (`URGENT_QUEUE` already wait, or the link is gone).
    pub(crate) fn send_urgent(&self, msg: NodeToMaster) -> bool {
        let mut l = self.lanes();
        if l.closed || l.urgent.len() >= URGENT_QUEUE {
            return false;
        }
        l.urgent.push_back(msg);
        drop(l);
        self.ready.notify_one();
        true
    }

    /// The next message for the link: an urgent one first, unless
    /// `URGENT_BURST` went in a row while ordered ones waited.
    fn pick(&self) -> Option<NodeToMaster> {
        let mut l = self.lanes();
        if !l.urgent.is_empty() && (l.ordered.is_empty() || l.run < URGENT_BURST) {
            if !l.ordered.is_empty() {
                l.run += 1;
            }
            return l.urgent.pop_front();
        }
        l.run = 0;
        l.ordered.pop_front().map(|q| q.msg)
    }

    async fn feed(self: Arc<Self>, tx: mpsc::Sender<NodeToMaster>) {
        loop {
            // Something to send first, then the slot, then the choice: what
            // goes next is decided when the link has taken the last one.
            let (empty, end) = {
                let l = self.lanes();
                let empty = l.ordered.is_empty() && l.urgent.is_empty();
                (empty, l.closed || (empty && l.retired))
            };
            if end {
                break;
            }
            if empty {
                tokio::select! {
                    _ = self.ready.notified() => {}
                    _ = tx.closed() => break,
                }
                continue;
            }
            let Ok(slot) = tx.reserve().await else { break };
            if let Some(m) = self.pick() {
                slot.send(m);
            }
        }
        self.close();
    }

    /// Nothing queued, and the link has taken what was handed to it (or is
    /// gone).
    pub(crate) fn is_drained(&self) -> bool {
        {
            let l = self.lanes();
            if l.closed {
                return true;
            }
            if !l.ordered.is_empty() || !l.urgent.is_empty() {
                return false;
            }
        }
        self.link
            .upgrade()
            .is_none_or(|tx| tx.is_closed() || tx.capacity() == tx.max_capacity())
    }

    pub(crate) fn backlog(&self) -> Backlog {
        let l = self.lanes();
        Backlog {
            queued: l.ordered.len(),
            bytes: OUT_BYTES.saturating_sub(self.bytes.available_permits()),
            urgent: l.urgent.len(),
            waiting: self.waiting.load(Ordering::SeqCst),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ack(req: &str) -> NodeToMaster {
        NodeToMaster::Ack {
            req_id: req.into(),
            ok: true,
            error: None,
        }
    }

    fn req(m: &NodeToMaster) -> &str {
        match m {
            NodeToMaster::Ack { req_id, .. } => req_id,
            _ => "",
        }
    }

    /// Urgent acks pass the queue, at most `URGENT_BURST` in a row while
    /// ordered messages wait; ordered ones keep their order.
    #[tokio::test]
    async fn urgent_acks_pass_the_queue_in_bursts() {
        let (o, mut rx) = Outbox::open();
        for i in 0..20 {
            assert!(o.try_send(ack(&format!("o{i}"))));
        }
        for i in 0..20 {
            assert!(o.send_urgent(ack(&format!("u{i}"))));
        }
        let mut got = vec![];
        for _ in 0..40 {
            got.push(req(&rx.recv().await.unwrap()).to_string());
        }
        // The feeder may have handed o0 to the link before the urgent ones
        // came; after that the pattern is fixed.
        let start = usize::from(got[0] == "o0");
        let ordered: Vec<_> = got.iter().filter(|g| g.starts_with('o')).collect();
        let expect: Vec<String> = (0..20).map(|i| format!("o{i}")).collect();
        assert_eq!(ordered, expect.iter().collect::<Vec<_>>());
        let first_ordered_after = got[start..].iter().position(|g| g.starts_with('o'));
        assert_eq!(first_ordered_after, Some(URGENT_BURST), "{got:?}");
    }

    /// The urgent lane never waits and keeps at most `URGENT_QUEUE`.
    #[tokio::test]
    async fn urgent_acks_past_the_lane_are_dropped() {
        let (o, _rx) = Outbox::open();
        let kept = (0..URGENT_QUEUE * 3)
            .filter(|i| o.send_urgent(ack(&format!("u{i}"))))
            .count();
        // One may be in the link's channel already.
        assert!((URGENT_QUEUE..=URGENT_QUEUE + 1).contains(&kept), "{kept}");
    }

    /// The ordered queue is bounded in bytes: a sender waits once
    /// `OUT_BYTES` are queued, and a message larger than that goes alone.
    #[tokio::test]
    async fn the_queue_is_bounded_in_bytes() {
        let (o, mut rx) = Outbox::open();
        let big = |r: &str| NodeToMaster::Ack {
            req_id: r.into(),
            ok: false,
            error: Some("x".repeat(OUT_BYTES / 4 - 100)),
        };
        // Four fill the bytes (the feeder has not run yet).
        let mut n = 0;
        while o.try_send(big(&format!("b{n}"))) {
            n += 1;
            assert!(n < 16, "not bounded in bytes");
        }
        assert!((4..=5).contains(&n), "{n} quarter-size messages queued");
        assert!(o.backlog().bytes <= OUT_BYTES);
        let huge = NodeToMaster::Ack {
            req_id: "huge".into(),
            ok: false,
            error: Some("x".repeat(OUT_BYTES * 2)),
        };
        let o2 = o.clone();
        let wait = tokio::spawn(async move { o2.send(huge).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(o.backlog().waiting, 1);
        for _ in 0..n {
            rx.recv().await.unwrap();
        }
        wait.await.unwrap();
        assert_eq!(req(&rx.recv().await.unwrap()), "huge");
    }

    /// A closed link lets every waiting sender go.
    #[tokio::test]
    async fn a_dropped_link_releases_waiting_senders() {
        let (o, rx) = Outbox::open();
        // One in the link's channel, then a full queue.
        assert!(o.try_send(ack("first")));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        while o.try_send(ack("x")) {}
        let o2 = o.clone();
        let wait = tokio::spawn(async move { o2.send(ack("y")).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(o.backlog().waiting, 1);
        drop(rx);
        tokio::time::timeout(std::time::Duration::from_secs(5), wait)
            .await
            .expect("a sender still waits after the link closed")
            .unwrap();
        assert!(o.is_closed());
        assert!(o.is_drained());
    }
}

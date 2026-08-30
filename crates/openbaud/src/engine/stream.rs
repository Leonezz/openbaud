//! Per-consumer frame subscriptions for the `stream_poll` tool (R-07).
//!
//! Each subscription is an independent incremental consumer of one session's
//! byte stream, with ack/redelivery semantics so a lost tool response is
//! recoverable. The four risk red lines and their resolutions:
//!
//! - Risk ① (never steal from `read`): a subscription owns its *own* ChunkBuf
//!   cursor, its own `Deframer` and its own retention queue, and reads the
//!   chunk buffer only through the non-destructive `ChunkBuf::since()`.
//!   `ReadState` — the `read` tool's cursor — is structurally out of reach
//!   from this module, so neither consumer can ever take the other's frames.
//! - Risk ② (bounded lag): retention is capped at [`MAX_RETAINED_FRAMES`]
//!   frames per subscription; overflow drops the oldest frame and increments
//!   the cumulative `dropped_frames` counter reported on every poll — never
//!   silent. Byte-level overflow of the shared chunk buffer is attributed to
//!   each subscription it actually hit: chunks dropped past a subscription's
//!   cursor are counted in its cumulative `dropped_chunks` (and a partial
//!   frame destroyed by the gap in `dropped_frames`), on top of the session
//!   `stats` snapshot's cumulative `dropped_bytes`.
//! - Risk ③ (abandonment): subscriptions live inside their [`Session`], so
//!   closing the session releases all of them; each carries `last_polled_ms`,
//!   and every stream call on the session sweeps subscriptions idle beyond
//!   [`SUBSCRIPTION_IDLE_TTL_MS`]; at most [`MAX_SUBSCRIPTIONS_PER_SESSION`]
//!   may exist, more is a loud error. Residual risk, stated honestly: a
//!   subscription nobody polls again survives until the next stream call on
//!   its session or session close — bounded memory (at most 8 subscriptions
//!   of at most 1024 retained frames each).
//! - Risk ④ (host approval/throttling for widget polling is unproven): this
//!   tool carries no `_meta.ui` binding and the skill docs do not steer
//!   widgets at it — it is a data tool for the agent.

use crate::engine::session::Session;
use anyhow::{anyhow, bail};
use openbaud_core::framing::{Deframer, Framing};
use openbaud_core::hex;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

/// Retention cap per subscription (risk ②): beyond this many unacknowledged
/// frames the oldest is dropped and counted in `dropped_frames`.
pub const MAX_RETAINED_FRAMES: usize = 1024;

/// Subscription cap per session (risk ③): a ninth subscribe is a loud error.
pub const MAX_SUBSCRIPTIONS_PER_SESSION: usize = 8;

/// Idle TTL (risk ③): subscriptions not polled for longer than this are swept
/// by any later stream call on their session.
pub const SUBSCRIPTION_IDLE_TTL_MS: u64 = 120_000;

/// Default and hard cap for `max_frames` in one poll result.
pub const DEFAULT_POLL_FRAMES: usize = 64;
pub const MAX_POLL_FRAMES: usize = 256;

/// Default and bounds for `max_inline_bytes` in one poll result: the page
/// budget metered as each frame's rendered `hex.len() + text.len()`. Idle-gap
/// merged frames can reach megabytes and hex+text rendering inflates ~4x, so
/// `max_frames` alone never bounded the payload — this does.
pub const DEFAULT_POLL_INLINE_BYTES: usize = 4096;
pub const MIN_POLL_INLINE_BYTES: usize = 512;
pub const MAX_POLL_INLINE_BYTES: usize = 262_144;

/// One deframed frame held for a consumer until it acknowledges delivery.
struct RetainedFrame {
    seq: u64,
    ts_ms: u64,
    bytes: Vec<u8>,
}

/// One consumer's private view of the session stream: cursor, deframer and
/// retention queue — no state shared with `ReadState` (risk ①).
struct Subscription {
    /// Private ChunkBuf cursor. When the shared buffer overflows past it the
    /// missed bytes are gone for this consumer too; each such gap is counted
    /// in this subscription's `dropped_chunks` (never silently), and the
    /// buffer's cumulative `dropped_bytes` is surfaced in every poll's
    /// `stats` (same source of truth the `read` tool reports deltas of).
    cursor: u64,
    deframer: Deframer,
    /// Wall-clock start of the frame currently pending in the deframer, with
    /// the same semantics as `pending_since_ms` in `ReadState`.
    pending_since_ms: Option<u64>,
    retained: VecDeque<RetainedFrame>,
    /// Seq the next deframed frame will get; monotonic per subscription.
    next_seq: u64,
    /// Seq one past the highest frame ever *delivered* in a poll result —
    /// the only frames an ack may release. `next_seq` can run ahead of this
    /// when a `max_frames`-capped page truncates delivery, so acks are
    /// validated against this watermark, not `next_seq`: acknowledging a
    /// frame the consumer never saw is a loud error, never a silent release.
    delivered_seq: u64,
    /// Frames dropped from the retention queue because the consumer lagged
    /// (risk ②) — cumulative, reported on every poll. Also counts a partial
    /// frame destroyed when shared-buffer overflow skipped past the cursor.
    dropped_frames: u64,
    /// Chunks the shared buffer dropped past this subscription's cursor
    /// (byte-level overflow, risk ②) — cumulative, reported on every poll.
    /// Frames wholly inside such a gap are uncountable, so this is the loud
    /// signal that `dropped_frames == 0` alone does not mean "no loss".
    dropped_chunks: u64,
    last_polled_ms: u64,
}

impl Subscription {
    fn new(framing: Framing, now_ms: u64) -> Self {
        Self {
            cursor: 0,
            deframer: Deframer::new(framing),
            pending_since_ms: None,
            retained: VecDeque::new(),
            next_seq: 0,
            delivered_seq: 0,
            dropped_frames: 0,
            dropped_chunks: 0,
            last_polled_ms: now_ms,
        }
    }

    /// Drain new chunks from the session buffer through this subscription's
    /// own deframer — the same consumption pattern as `read_frames`, but pure
    /// poll: no waiting, whatever is complete right now.
    fn drain(&mut self, session: &Session, now_ms: u64) {
        let (chunks, new_cursor, skipped) = session.chunks_since(self.cursor);
        if skipped > 0 {
            // Risk ②, byte-overflow flavor: the shared buffer dropped chunks
            // past this cursor, so this consumer lost data — count it here,
            // attributed to this subscription, never only in the shared
            // session-wide dropped_bytes.
            self.dropped_chunks += skipped;
            // A partial frame pending in the deframer can never complete
            // correctly across the gap; splicing pre-gap and post-gap bytes
            // into one frame would be silent corruption. Destroy it and count
            // the loss as a dropped frame.
            if self.deframer.pending_len() > 0 {
                let _ = self.deframer.flush_pending();
                self.dropped_frames += 1;
            }
            self.pending_since_ms = None;
        }
        self.cursor = new_cursor;
        for (ts, bytes) in chunks {
            if self.pending_since_ms.is_none() {
                self.pending_since_ms = Some(ts);
            }
            for frame in self.deframer.push(&bytes) {
                self.retain(ts, frame);
            }
            if self.deframer.pending_len() == 0 {
                self.pending_since_ms = None;
            }
        }
        // Idle-gap framing: flush pending once the wire has been quiet.
        if let Framing::Idle { idle_ms } = self.deframer.framing() {
            let idle_ms = *idle_ms;
            if self.deframer.pending_len() > 0
                && now_ms.saturating_sub(session.last_rx()) >= idle_ms
            {
                if let Some(pending) = self.deframer.flush_pending() {
                    let ts = self.pending_since_ms.take().unwrap_or(now_ms);
                    self.retain(ts, pending);
                }
            }
        }
    }

    fn retain(&mut self, ts_ms: u64, bytes: Vec<u8>) {
        self.retained.push_back(RetainedFrame { seq: self.next_seq, ts_ms, bytes });
        self.next_seq += 1;
        // Risk ②: bounded retention — a lagging consumer loses the oldest
        // frames, and the loss is counted, never silent.
        while self.retained.len() > MAX_RETAINED_FRAMES {
            self.retained.pop_front();
            self.dropped_frames += 1;
        }
    }

    /// Ack: release delivered frames with seq below `since_seq`. A since_seq
    /// pointing into the dropped range releases nothing — redelivery starts
    /// from the oldest *retained* frame, and `dropped_frames` keeps saying
    /// what was lost.
    fn release_before(&mut self, since_seq: u64) {
        while self.retained.front().is_some_and(|f| f.seq < since_seq) {
            self.retained.pop_front();
        }
    }

    fn idle_longer_than(&self, ttl_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_polled_ms) > ttl_ms
    }
}

/// All of one session's subscriptions. Lives inside the `Session` (risk ③:
/// session close drops everything here).
#[derive(Default)]
pub struct SubscriptionSet {
    subs: HashMap<String, Subscription>,
    next_id: u64,
}

impl SubscriptionSet {
    /// Remove every subscription idle beyond the TTL; returns the swept ids,
    /// sorted for stable output.
    fn sweep(&mut self, now_ms: u64) -> Vec<String> {
        let mut expired: Vec<String> = self
            .subs
            .iter()
            .filter(|(_, sub)| sub.idle_longer_than(SUBSCRIPTION_IDLE_TTL_MS, now_ms))
            .map(|(id, _)| id.clone())
            .collect();
        expired.sort();
        for id in &expired {
            self.subs.remove(id);
        }
        expired
    }
}

impl Session {
    /// Create a subscription starting from the oldest still-buffered chunk.
    /// Sweeps idle subscriptions first (risk ③) and refuses to exceed the
    /// per-session cap. `now_ms` is a parameter (not read inside) so tests can
    /// drive the TTL with a fabricated clock.
    pub fn stream_subscribe(&self, now_ms: u64) -> anyhow::Result<String> {
        let mut set = self.subs.lock().unwrap();
        set.sweep(now_ms);
        if set.subs.len() >= MAX_SUBSCRIPTIONS_PER_SESSION {
            let mut ids: Vec<String> = set.subs.keys().cloned().collect();
            ids.sort();
            bail!(
                "session {} already has {MAX_SUBSCRIPTIONS_PER_SESSION} stream subscriptions \
                 ([{}]) — close one (stream_poll with close: true) before subscribing again",
                self.id,
                ids.join(", ")
            );
        }
        set.next_id += 1;
        let id = format!("{}-sub{}", self.id, set.next_id);
        set.subs.insert(id.clone(), Subscription::new(self.framing().clone(), now_ms));
        Ok(id)
    }

    /// One poll: drain new frames, apply the optional ack, return the oldest
    /// unacknowledged frames — at most `max_frames` of them and at most
    /// `max_inline_bytes` of rendered payload (metered as each frame's
    /// `hex.len() + text.len()`), whole frames only — plus the folded-in
    /// session stats. A first frame alone exceeding the byte budget is still
    /// delivered (forward progress), alone, with `oversized_frame: true`.
    /// Pure poll — returns immediately with whatever is available.
    pub fn stream_pull(
        &self,
        sub_id: &str,
        since_seq: Option<u64>,
        max_frames: usize,
        max_inline_bytes: usize,
        now_ms: u64,
    ) -> anyhow::Result<Value> {
        let mut set = self.subs.lock().unwrap();
        // Risk ③: every pull sweeps this session's idle subscriptions — and a
        // pull of a subscription that itself sat idle past the TTL is a loud
        // expiry, not a silent revival.
        let expired_self =
            set.subs.get(sub_id).is_some_and(|s| s.idle_longer_than(SUBSCRIPTION_IDLE_TTL_MS, now_ms));
        set.sweep(now_ms);
        if expired_self {
            bail!(
                "stream subscription {sub_id:?} on session {} expired: idle longer than \
                 {SUBSCRIPTION_IDLE_TTL_MS} ms — subscribe again with session_id",
                self.id
            );
        }
        let sub = set
            .subs
            .get_mut(sub_id)
            .ok_or_else(|| anyhow!("no stream subscription {sub_id:?} on session {}", self.id))?;
        sub.drain(self, now_ms);
        if let Some(seq) = since_seq {
            // Validate against the delivered watermark, not `next_seq`:
            // frames can be drained (and counted in next_seq) without ever
            // having been returned to the consumer — a max_frames-capped page
            // truncates delivery — and releasing those silently would destroy
            // frames nobody saw.
            if seq > sub.delivered_seq {
                bail!(
                    "since_seq {seq} acknowledges frames that were never delivered \
                     (highest deliverable ack is {}; next_seq is {}) — ack with the \
                     last delivered frame's seq + 1",
                    sub.delivered_seq,
                    sub.next_seq
                );
            }
            sub.release_before(seq);
        }
        // Build the page frame by frame under both caps: `max_frames` and the
        // `max_inline_bytes` budget (metered as rendered hex + text length).
        // Only whole frames are ever inlined; a first frame that alone blows
        // the budget is still delivered — alone — so a stream of oversized
        // frames (e.g. idle-merged megabyte frames) always makes progress,
        // and the page says so via `oversized_frame`.
        let mut frames: Vec<Value> = Vec::new();
        let mut page_bytes: usize = 0;
        let mut oversized_frame = false;
        for f in sub.retained.iter().take(max_frames) {
            let hex = hex::to_hex(&f.bytes);
            let text = hex::to_text_lossy(&f.bytes);
            let frame_bytes = hex.len() + text.len();
            if !frames.is_empty() && page_bytes + frame_bytes > max_inline_bytes {
                break;
            }
            oversized_frame = frames.is_empty() && frame_bytes > max_inline_bytes;
            page_bytes += frame_bytes;
            frames.push(json!({
                "seq": f.seq,
                "ts_ms": f.ts_ms,
                "hex": hex,
                "text": text,
            }));
            if oversized_frame {
                break;
            }
        }
        let delivered = frames.len();
        if delivered > 0 {
            sub.delivered_seq = sub.delivered_seq.max(sub.retained[delivered - 1].seq + 1);
        }
        sub.last_polled_ms = now_ms;
        if frames.is_empty() {
            // Mirror read_frames' rule: surface a dead port only once nothing
            // retained is left to deliver — a consumer polling only through
            // this tool must never see an endless stream of healthy-looking
            // empty results from a failed session.
            self.check_alive()?;
        }
        let (next_seq, dropped_frames, dropped_chunks) =
            (sub.next_seq, sub.dropped_frames, sub.dropped_chunks);
        drop(set);
        Ok(json!({
            "subscription_id": sub_id,
            "session_id": self.id,
            "frames": frames,
            "next_seq": next_seq,
            "page_bytes": page_bytes,
            "oversized_frame": oversized_frame,
            "dropped_frames": dropped_frames,
            "dropped_chunks": dropped_chunks,
            "stats": self.stream_stats(),
        }))
    }

    /// Explicitly release one subscription. Closing an unknown (or already
    /// swept) id is a loud error, not a no-op.
    pub fn stream_close(&self, sub_id: &str, now_ms: u64) -> anyhow::Result<Value> {
        let mut set = self.subs.lock().unwrap();
        set.subs
            .remove(sub_id)
            .ok_or_else(|| anyhow!("no stream subscription {sub_id:?} on session {} to close", self.id))?;
        set.sweep(now_ms);
        drop(set);
        Ok(json!({
            "subscription_id": sub_id,
            "session_id": self.id,
            "closed": true,
            "stats": self.stream_stats(),
        }))
    }

    pub fn has_stream_subscription(&self, sub_id: &str) -> bool {
        self.subs.lock().unwrap().subs.contains_key(sub_id)
    }

    /// Sweep idle subscriptions at a caller-supplied instant; returns the
    /// swept ids. Public as the TTL test seam — tests fabricate `now_ms`
    /// instead of sleeping out the 120 s TTL.
    pub fn sweep_stream_subscriptions_at(&self, now_ms: u64) -> Vec<String> {
        self.subs.lock().unwrap().sweep(now_ms)
    }
}

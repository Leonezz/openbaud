//! Deframing of continuous byte streams into frames.
//!
//! `Deframer` is a pure push-based state machine for delimiter and
//! length-prefix framing. Idle-gap framing depends on time, which this crate
//! does not own: `TimedDeframer` takes each chunk's arrival timestamp as
//! input, so every consumer — live read, stream subscription, offline
//! capture re-framing — cuts idle frames by the same rule.

#[derive(Debug, Clone, PartialEq)]
pub enum Framing {
    /// Frame ends with this byte sequence (delimiter stripped from the frame).
    Delimiter { delimiter: Vec<u8> },
    /// Frame boundary = receive gap of at least `idle_ms` (flushed by caller).
    Idle { idle_ms: u64 },
    /// Header of `header_len` bytes carries a payload length at `len_at`
    /// (`len_size` bytes, big- or little-endian). Total frame length =
    /// header_len + payload_len + extra.
    LengthPrefix {
        header_len: usize,
        len_at: usize,
        len_size: usize,
        big_endian: bool,
        extra: usize,
    },
}

/// Cap on unframed pending bytes; beyond this the buffer is force-flushed as a
/// frame so a missing delimiter can't grow memory without bound.
const MAX_PENDING: usize = 64 * 1024;

#[derive(Debug)]
pub struct Deframer {
    framing: Framing,
    buf: Vec<u8>,
}

impl Deframer {
    pub fn new(framing: Framing) -> Self {
        Self { framing, buf: Vec::new() }
    }

    /// Feed received bytes; returns zero or more completed frames.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            let frame = match &self.framing {
                Framing::Delimiter { delimiter } => take_delimited(&mut self.buf, delimiter),
                Framing::LengthPrefix { header_len, len_at, len_size, big_endian, extra } => {
                    take_length_prefixed(&mut self.buf, *header_len, *len_at, *len_size, *big_endian, *extra)
                }
                Framing::Idle { .. } => None,
            };
            match frame {
                Some(f) => frames.push(f),
                None => break,
            }
        }
        if self.buf.len() > MAX_PENDING {
            frames.push(std::mem::take(&mut self.buf));
        }
        frames
    }

    /// Emit whatever is pending as one frame (idle-gap boundary or shutdown).
    pub fn flush_pending(&mut self) -> Option<Vec<u8>> {
        if self.buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buf))
        }
    }

    pub fn pending_len(&self) -> usize {
        self.buf.len()
    }

    pub fn framing(&self) -> &Framing {
        &self.framing
    }
}

/// A frame cut from a timestamped byte stream.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedFrame {
    pub ts_ms: u64,
    pub bytes: Vec<u8>,
}

/// A `Deframer` fed with chunk arrival times.
///
/// Idle-gap boundaries are a property of the stream: a frame ends where two
/// consecutive chunks arrived at least `idle_ms` apart. They must never
/// depend on when a consumer happens to drain the buffer — a consumer that
/// reads late would otherwise see every buffered chunk merged into one frame.
/// The only boundary that needs the current time is the tail, closed by
/// `flush_if_quiet` once the wire has been silent since the last chunk.
///
/// Frame timestamps: a delimiter or length-prefix frame carries the arrival
/// time of the chunk that completed it; an idle frame carries the arrival
/// time of its first chunk.
#[derive(Debug)]
pub struct TimedDeframer {
    deframer: Deframer,
    pending_since: Option<u64>,
    last_ts: Option<u64>,
}

impl TimedDeframer {
    pub fn new(framing: Framing) -> Self {
        Self { deframer: Deframer::new(framing), pending_since: None, last_ts: None }
    }

    /// Feed one chunk received at `ts_ms`; returns the frames it completed,
    /// including an idle frame closed by the gap before this chunk.
    pub fn push(&mut self, ts_ms: u64, bytes: &[u8]) -> Vec<TimedFrame> {
        let mut out = Vec::new();
        if let (Some(idle_ms), Some(last)) = (self.idle_ms(), self.last_ts) {
            if ts_ms.saturating_sub(last) >= idle_ms {
                out.extend(self.take_pending());
            }
        }
        if self.pending_since.is_none() {
            self.pending_since = Some(ts_ms);
        }
        out.extend(self.deframer.push(bytes).into_iter().map(|bytes| TimedFrame { ts_ms, bytes }));
        if self.deframer.pending_len() == 0 {
            self.pending_since = None;
        }
        self.last_ts = Some(ts_ms);
        out
    }

    /// Close the pending idle frame if the wire has been quiet for at least
    /// `idle_ms` since the last chunk. No-op for other framings.
    pub fn flush_if_quiet(&mut self, now_ms: u64) -> Option<TimedFrame> {
        let idle_ms = self.idle_ms()?;
        let last = self.last_ts?;
        if now_ms.saturating_sub(last) >= idle_ms {
            self.take_pending()
        } else {
            None
        }
    }

    /// End of stream: under idle framing, whatever is pending is a complete
    /// frame. Other framings leave a partial frame pending (see `pending_len`).
    pub fn finish(&mut self) -> Option<TimedFrame> {
        self.idle_ms()?;
        self.take_pending()
    }

    /// Destroy a partial frame that can never complete correctly (the bytes
    /// before or after it were lost). Returns whether anything was dropped.
    pub fn discard_pending(&mut self) -> bool {
        self.pending_since = None;
        self.deframer.flush_pending().is_some()
    }

    pub fn pending_len(&self) -> usize {
        self.deframer.pending_len()
    }

    pub fn framing(&self) -> &Framing {
        self.deframer.framing()
    }

    fn idle_ms(&self) -> Option<u64> {
        match self.deframer.framing() {
            Framing::Idle { idle_ms } => Some(*idle_ms),
            _ => None,
        }
    }

    fn take_pending(&mut self) -> Option<TimedFrame> {
        let bytes = self.deframer.flush_pending()?;
        let ts_ms = self.pending_since.take().or(self.last_ts).unwrap_or(0);
        Some(TimedFrame { ts_ms, bytes })
    }
}

fn take_delimited(buf: &mut Vec<u8>, delimiter: &[u8]) -> Option<Vec<u8>> {
    if delimiter.is_empty() {
        return None;
    }
    let pos = buf
        .windows(delimiter.len())
        .position(|w| w == delimiter)?;
    let frame = buf[..pos].to_vec();
    buf.drain(..pos + delimiter.len());
    Some(frame)
}

fn take_length_prefixed(
    buf: &mut Vec<u8>,
    header_len: usize,
    len_at: usize,
    len_size: usize,
    big_endian: bool,
    extra: usize,
) -> Option<Vec<u8>> {
    if buf.len() < header_len {
        return None;
    }
    let len_bytes = buf.get(len_at..len_at + len_size)?;
    let payload_len = if big_endian {
        len_bytes.iter().fold(0usize, |acc, &b| (acc << 8) | b as usize)
    } else {
        len_bytes.iter().rev().fold(0usize, |acc, &b| (acc << 8) | b as usize)
    };
    let total = header_len + payload_len + extra;
    if buf.len() < total {
        return None;
    }
    let frame = buf[..total].to_vec();
    buf.drain(..total);
    Some(frame)
}

/// Response matcher for request/response commands: accumulates bytes until the
/// match rule is satisfied. Idle matching is caller-timed, like `Deframer`.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchRule {
    Length(usize),
    Delimiter(Vec<u8>),
    Idle { idle_ms: u64 },
}

#[derive(Debug)]
pub struct Matcher {
    rule: MatchRule,
    buf: Vec<u8>,
}

impl Matcher {
    pub fn new(rule: MatchRule) -> Self {
        Self { rule, buf: Vec::new() }
    }

    /// Feed bytes; returns the completed response once the rule is satisfied.
    /// Excess bytes beyond a Length rule stay buffered (returned frame is exact).
    pub fn push(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        self.buf.extend_from_slice(bytes);
        match &self.rule {
            MatchRule::Length(n) => {
                if self.buf.len() >= *n {
                    let frame = self.buf[..*n].to_vec();
                    self.buf.drain(..*n);
                    Some(frame)
                } else {
                    None
                }
            }
            MatchRule::Delimiter(delim) => take_delimited(&mut self.buf, delim),
            MatchRule::Idle { .. } => None,
        }
    }

    pub fn rule(&self) -> &MatchRule {
        &self.rule
    }

    pub fn take_pending(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiter_split_across_pushes() {
        let mut d = Deframer::new(Framing::Delimiter { delimiter: b"\r\n".to_vec() });
        assert!(d.push(b"$GPGGA,12").is_empty());
        let frames = d.push(b"3\r\n$GPRMC\r\npartial");
        assert_eq!(frames, vec![b"$GPGGA,123".to_vec(), b"$GPRMC".to_vec()]);
        assert_eq!(d.pending_len(), 7);
        assert_eq!(d.flush_pending().unwrap(), b"partial".to_vec());
    }

    #[test]
    fn length_prefix_modbus_style() {
        // header: addr fc len | payload | crc(2) => header_len=3, len_at=2, extra=2
        let mut d = Deframer::new(Framing::LengthPrefix {
            header_len: 3,
            len_at: 2,
            len_size: 1,
            big_endian: true,
            extra: 2,
        });
        let frame = [0x01, 0x04, 0x02, 0x08, 0x9B, 0xAA, 0xBB];
        assert!(d.push(&frame[..4]).is_empty());
        let frames = d.push(&frame[4..]);
        assert_eq!(frames, vec![frame.to_vec()]);
    }

    #[test]
    fn matcher_length_exact_with_excess() {
        let mut m = Matcher::new(MatchRule::Length(3));
        assert!(m.push(&[1, 2]).is_none());
        assert_eq!(m.push(&[3, 4]).unwrap(), vec![1, 2, 3]);
        assert_eq!(m.take_pending(), vec![4]);
    }

    fn texts(frames: &[TimedFrame]) -> Vec<(u64, String)> {
        frames.iter().map(|f| (f.ts_ms, String::from_utf8_lossy(&f.bytes).into_owned())).collect()
    }

    #[test]
    fn idle_gap_between_chunks_closes_a_frame_regardless_of_drain_time() {
        // All chunks pushed in one batch, long after they arrived — the
        // situation of a consumer that reads late. Boundaries come from the
        // arrival times alone.
        let mut d = TimedDeframer::new(Framing::Idle { idle_ms: 100 });
        let mut out = Vec::new();
        out.extend(d.push(0, b"A"));
        out.extend(d.push(10, b"1"));
        out.extend(d.push(250, b"B2"));
        out.extend(d.push(500, b"C3"));
        assert_eq!(texts(&out), vec![(0, "A1".to_string()), (250, "B2".to_string())]);
        assert_eq!(d.pending_len(), 2);
    }

    #[test]
    fn idle_tail_waits_for_quiet_then_flushes_once() {
        let mut d = TimedDeframer::new(Framing::Idle { idle_ms: 100 });
        assert!(d.push(1000, b"tail").is_empty());
        assert!(d.flush_if_quiet(1099).is_none(), "not quiet long enough yet");
        let f = d.flush_if_quiet(1100).expect("quiet for idle_ms");
        assert_eq!((f.ts_ms, f.bytes), (1000, b"tail".to_vec()));
        assert!(d.flush_if_quiet(5000).is_none(), "nothing left to flush");
    }

    #[test]
    fn gaps_never_cut_delimiter_or_length_prefixed_frames() {
        let mut d = TimedDeframer::new(Framing::Delimiter { delimiter: b"\n".to_vec() });
        assert!(d.push(0, b"par").is_empty());
        assert!(d.push(10_000, b"tial").is_empty(), "a gap is not a boundary here");
        assert!(d.flush_if_quiet(99_999).is_none());
        assert!(d.finish().is_none(), "a partial delimited frame is not complete");
        assert_eq!(texts(&d.push(10_001, b"\n")), vec![(10_001, "partial".to_string())]);
    }

    #[test]
    fn finish_completes_only_an_idle_frame() {
        let mut d = TimedDeframer::new(Framing::Idle { idle_ms: 100 });
        d.push(7, b"end");
        assert_eq!(d.finish().map(|f| (f.ts_ms, f.bytes)), Some((7, b"end".to_vec())));
    }

    #[test]
    fn discard_pending_drops_a_partial_frame_and_its_start_time() {
        let mut d = TimedDeframer::new(Framing::Idle { idle_ms: 100 });
        d.push(0, b"lost");
        assert!(d.discard_pending());
        assert!(!d.discard_pending(), "nothing left");
        d.push(50, b"new");
        let f = d.finish().unwrap();
        assert_eq!((f.ts_ms, f.bytes), (50, b"new".to_vec()), "start time restarts after a discard");
    }

    #[test]
    fn runaway_pending_is_force_flushed() {
        let mut d = Deframer::new(Framing::Delimiter { delimiter: b"\n".to_vec() });
        let big = vec![0u8; MAX_PENDING + 1];
        let frames = d.push(&big);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), MAX_PENDING + 1);
        assert_eq!(d.pending_len(), 0);
    }
}

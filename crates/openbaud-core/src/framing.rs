//! Deframing of continuous byte streams into frames.
//!
//! `Deframer` is a pure push-based state machine for delimiter and
//! length-prefix framing. Idle-gap framing depends on wall-clock time, which
//! this crate does not own: the IO layer detects the gap and calls
//! `flush_pending()`.

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

//! The four data tools that read back what already happened: session_timeline
//! and capture_frames over recorded captures, diagnose_frame over one raw
//! frame, session_stats over live sessions. All read-only — nothing here
//! touches the wire and nothing is audited. Results that describe a drawable
//! shape carry an explicit `view` declaration, so handing them (or their
//! `full_result` path) to `show_result` renders them; nothing is ever guessed
//! from field names.

use crate::mcp::tools::{arg_max_inline, arg_str, arg_u64_or};
use crate::mcp::Ctx;
use crate::output::{self, shape_result};
use crate::workspace::flat_file_name;
use anyhow::{anyhow, bail, Context};
use openbaud_core::checksum::ChecksumKind;
use openbaud_core::exec::{parse_with_spec, verify_checksum};
use openbaud_core::format::{Encoding, FramingSpec, ParseSpec, ValidateSpec};
use openbaud_core::framing::{Deframer, Framing};
use openbaud_core::hex;
use openbaud_core::CoreError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Workspace-relative directory the capture tools will read from — and the
/// only one.
const CAPTURES_DIR: &str = "captures";

/// Resolve a caller-supplied capture path ("captures/<file>.obcap", optional
/// leading "./"; or an absolute path that canonicalizes into the workspace's
/// captures/ directory) to the absolute file plus its normalized relative
/// form, refusing anything nested or escaping.
fn capture_file(path: &str, root: &Path) -> anyhow::Result<(PathBuf, String)> {
    let trimmed = path.strip_prefix("./").unwrap_or(path);
    if Path::new(trimmed).is_absolute() {
        return absolute_capture_file(trimmed, root);
    }
    let rest = trimmed
        .strip_prefix(CAPTURES_DIR)
        .and_then(|r| r.strip_prefix('/'))
        .ok_or_else(|| {
            anyhow!("path must be a capture under {CAPTURES_DIR}/ (e.g. captures/cap-….obcap)")
        })?;
    let name = flat_file_name(rest, CAPTURES_DIR)?;
    Ok((root.join(CAPTURES_DIR).join(name), format!("{CAPTURES_DIR}/{name}")))
}

/// Accept an absolute capture path only when it provably canonicalizes to a
/// file directly inside the workspace's captures/ directory — symlink tricks
/// and traversal land outside it and are refused loudly.
fn absolute_capture_file(path: &str, root: &Path) -> anyhow::Result<(PathBuf, String)> {
    let canon = Path::new(path)
        .canonicalize()
        .with_context(|| format!("cannot resolve absolute capture path {path:?}"))?;
    let captures = root.join(CAPTURES_DIR);
    let captures = captures.canonicalize().with_context(|| {
        format!("cannot resolve the workspace {CAPTURES_DIR}/ directory {captures:?}")
    })?;
    if canon.parent() != Some(captures.as_path()) {
        bail!(
            "absolute path {path:?} is not a file directly inside the workspace \
             {CAPTURES_DIR}/ directory ({})",
            captures.display()
        );
    }
    let name = canon
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("absolute path {path:?} carries no file name"))?;
    let name = flat_file_name(name, CAPTURES_DIR)?;
    Ok((canon.clone(), format!("{CAPTURES_DIR}/{name}")))
}

// ---------------------------------------------------------------------------
// Reading .obcap captures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Tx,
    Rx,
}

impl Dir {
    fn name(self) -> &'static str {
        match self {
            Dir::Tx => "tx",
            Dir::Rx => "rx",
        }
    }
}

struct ObcapRecord {
    ts_ms: u64,
    dir: Dir,
    bytes: Vec<u8>,
}

struct Obcap {
    session: String,
    port: String,
    started_ms: u64,
    records: Vec<ObcapRecord>,
}

/// Parse a capture file: obcap:1 header line, then one JSON record per line.
/// Every malformed line is a loud error naming the line — a capture this tool
/// cannot fully read is never partially summarized.
fn read_obcap(file: &Path, rel: &str) -> anyhow::Result<Obcap> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("cannot read capture {rel:?}"))?;
    let mut lines = content.lines().enumerate();
    let (_, first) = lines
        .next()
        .ok_or_else(|| anyhow!("capture {rel:?} is empty — expected an obcap header line"))?;
    let header: Value = serde_json::from_str(first)
        .with_context(|| format!("capture {rel:?}: first line is not JSON"))?;
    if header.get("obcap") != Some(&json!(1)) {
        bail!("capture {rel:?} is not an obcap:1 capture (header line: {first})");
    }
    let header_str = |key: &str| -> anyhow::Result<String> {
        header
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("capture {rel:?}: header has no string {key:?}"))
    };
    let session = header_str("session")?;
    let port = header_str("port")?;
    let started_ms = header
        .get("started_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("capture {rel:?}: header has no numeric started_ms"))?;

    let mut records = Vec::new();
    for (i, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = i + 1;
        let rec: Value = serde_json::from_str(line)
            .with_context(|| format!("capture {rel:?} line {line_no}: not a valid obcap record"))?;
        let ts_ms = rec
            .get("ts_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("capture {rel:?} line {line_no}: no numeric ts_ms"))?;
        let dir = match rec.get("dir").and_then(Value::as_str) {
            Some("tx") => Dir::Tx,
            Some("rx") => Dir::Rx,
            other => bail!(
                "capture {rel:?} line {line_no}: dir is {other:?}, expected \"tx\" or \"rx\""
            ),
        };
        let hex_str = rec
            .get("hex")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("capture {rel:?} line {line_no}: no hex field"))?;
        let bytes = hex::parse_hex(hex_str)
            .with_context(|| format!("capture {rel:?} line {line_no}: bad hex"))?;
        if bytes.is_empty() {
            continue; // zero-byte records carry nothing to bucket or frame
        }
        records.push(ObcapRecord { ts_ms, dir, bytes });
    }
    Ok(Obcap { session, port, started_ms, records })
}

// ---------------------------------------------------------------------------
// session_timeline
// ---------------------------------------------------------------------------

pub fn session_timeline(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    // v1 reads captures only; refusing a live session id loudly beats quietly
    // returning a timeline of nothing.
    if args.get("session_id").is_some_and(|v| !v.is_null()) {
        bail!("record a capture first — timeline needs a capture file");
    }
    let (file, rel) = capture_file(arg_str(args, "path")?, &ctx.workspace.root)?;
    let cap = read_obcap(&file, &rel)?;
    let buckets = arg_u64_or(args, "buckets", 200)?;
    if buckets == 0 {
        bail!("buckets must be >= 1");
    }
    let max_inline = arg_max_inline(args)?;

    let all_events = audit_events(&ctx.workspace.root, &cap.session)?;

    // Natural span: the capture's own range — header start through its last
    // record. Never derived from audit events: session ids restart at s1
    // every process, so the audit log holds same-named events from other
    // days that would stretch the span (and the buckets) absurdly. Events
    // are clipped to the span instead.
    let natural_from = cap.started_ms;
    let natural_to = cap
        .records
        .iter()
        .map(|r| r.ts_ms)
        .max()
        .unwrap_or(cap.started_ms)
        .max(cap.started_ms);
    let from_ms = arg_u64_or(args, "from_ms", natural_from)?;
    // A capture whose whole life fits one millisecond still gets a 1 ms axis:
    // the window may exceed the data, the data never exceeds the window.
    let to_ms = arg_u64_or(args, "to_ms", natural_to.max(from_ms + 1))?;
    if to_ms <= from_ms {
        bail!("window is empty: to_ms {to_ms} is not after from_ms {from_ms}");
    }

    let bucket_ms = (to_ms - from_ms).div_ceil(buckets).max(1);
    let mut folded: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    for rec in &cap.records {
        if rec.ts_ms < from_ms || rec.ts_ms > to_ms {
            continue;
        }
        let t0 = from_ms + (rec.ts_ms - from_ms) / bucket_ms * bucket_ms;
        let slot = folded.entry(t0).or_insert((0, 0));
        match rec.dir {
            Dir::Tx => slot.0 += rec.bytes.len() as u64,
            Dir::Rx => slot.1 += rec.bytes.len() as u64,
        }
    }
    let density: Vec<Value> = folded
        .into_iter()
        .map(|(t0, (tx, rx))| json!({ "t0": t0, "tx_bytes": tx, "rx_bytes": rx }))
        .collect();
    let events: Vec<Value> = all_events
        .into_iter()
        .filter(|(ts, _)| *ts >= from_ms && *ts <= to_ms)
        .map(|(_, event)| event)
        .collect();

    let result = json!({
        "span": { "from_ms": from_ms, "to_ms": to_ms },
        "bucket_ms": bucket_ms,
        "events": events,
        "density": density,
        "source": { "path": rel },
        "port": cap.port,
        "view": { "kind": "timeline" },
    });
    shape_result(result, &ctx.workspace.root, "session_timeline", max_inline)
}

/// Which timeline event an audit entry becomes. Entries outside this
/// vocabulary (none exist today) carry no event kind and are not drawn.
fn event_kind(entry: &Value) -> Option<&'static str> {
    if entry.get("denied") == Some(&json!(true)) {
        return Some("deny");
    }
    match entry.get("tool").and_then(Value::as_str) {
        Some("run_command") => Some("cmd"),
        Some("send") | Some("request") => Some("write"),
        Some("run_workflow") => Some("workflow"),
        Some("run_workflow.step") => Some("workflow_step"),
        _ => None,
    }
}

/// The audit entries of one session as (ts_ms, timeline event) pairs. A
/// missing audit log means no writes ever happened — an empty event list, not
/// an error; a malformed line is loud.
fn audit_events(root: &Path, session: &str) -> anyhow::Result<Vec<(u64, Value)>> {
    let path = root.join(".openbaud/audit.jsonl");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(line)
            .with_context(|| format!(".openbaud/audit.jsonl line {}: not valid JSON", i + 1))?;
        if entry.get("session").and_then(Value::as_str) != Some(session) {
            continue;
        }
        let Some(ts_ms) = entry.get("ts_ms").and_then(Value::as_u64) else { continue };
        let Some(kind) = event_kind(&entry) else { continue };
        let Some(tool) = entry.get("tool").and_then(Value::as_str) else { continue };
        let Some(ok) = entry.get("ok").and_then(Value::as_bool) else { continue };
        let mut event = Map::new();
        event.insert("ts_ms".to_string(), json!(ts_ms));
        event.insert("kind".to_string(), json!(kind));
        event.insert("tool".to_string(), json!(tool));
        event.insert("ok".to_string(), json!(ok));
        for key in ["command", "workflow", "outcome", "detail"] {
            if let Some(v) = entry.get(key).and_then(Value::as_str) {
                event.insert(key.to_string(), json!(v));
            }
        }
        out.push((ts_ms, Value::Object(event)));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// capture_frames
// ---------------------------------------------------------------------------

pub fn capture_frames(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let (file, rel) = capture_file(arg_str(args, "path")?, &ctx.workspace.root)?;
    let framing = frames_framing(args, ctx)?;
    let max_inline = arg_max_inline(args)?;
    let cursor = arg_u64_or(args, "cursor", 0)? as usize;
    let max_frames = (arg_u64_or(args, "max_frames", 256)? as usize).max(1);
    let from_ms = arg_u64_or(args, "from_ms", 0)?;
    let to_ms = arg_u64_or(args, "to_ms", u64::MAX)?;
    let cap = read_obcap(&file, &rel)?;

    let (frames, unframed) = deframe_capture(&cap.records, &framing);
    let in_window: Vec<&CapFrame> =
        frames.iter().filter(|f| f.ts_ms >= from_ms && f.ts_ms <= to_ms).collect();
    let total = in_window.len();
    let page: Vec<Value> = in_window
        .iter()
        .skip(cursor)
        .take(max_frames)
        .map(|f| {
            json!({
                "seq": f.seq,
                "ts_ms": f.ts_ms,
                "dir": f.dir.name(),
                "hex": hex::to_hex(&f.bytes),
                "len": f.bytes.len(),
            })
        })
        .collect();
    let consumed = cursor.saturating_add(page.len()).min(total);

    let mut result = json!({
        "header": { "port": cap.port, "started_ms": cap.started_ms },
        "frames": page,
        "total_in_window": total,
        "source": { "path": rel },
        "view": { "kind": "capture" },
    });
    let obj = result.as_object_mut().expect("result is an object");
    if consumed < total {
        obj.insert("next_cursor".to_string(), json!(consumed));
    }
    // Bytes at the end of the capture that never completed a frame under this
    // framing — reported, never silently dropped.
    if unframed.0 > 0 || unframed.1 > 0 {
        obj.insert(
            "unframed_tail".to_string(),
            json!({ "tx_bytes": unframed.0, "rx_bytes": unframed.1 }),
        );
    }
    shape_result(result, &ctx.workspace.root, "capture_frames", max_inline)
}

/// The framing to deframe a capture with: an explicit spec, or the named
/// device's profile framing. Both absent is a loud error — a silently assumed
/// framing would produce authoritative-looking wrong frames.
fn frames_framing(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Framing> {
    if let Some(spec) = args.get("framing").filter(|v| !v.is_null()) {
        let spec: FramingSpec = serde_json::from_value(spec.clone())
            .map_err(|e| anyhow!("invalid framing spec: {e}"))?;
        return Ok(spec.to_framing("capture_frames.framing")?);
    }
    match args.get("device").and_then(Value::as_str) {
        Some(name) => {
            let device = ctx.workspace.load_device(name)?;
            match &device.profile.framing {
                Some(spec) => Ok(spec.to_framing(&format!("devices/{name}/profile.yaml"))?),
                None => bail!(
                    "device {name:?} declares no framing in its profile — pass an explicit \
                     framing ({{delimiter}}|{{idle_ms}}|{{length_prefix}})"
                ),
            }
        }
        None => bail!(
            "no framing given — pass a framing object ({{delimiter}}|{{idle_ms}}|{{length_prefix}}) \
             or a device whose profile declares one"
        ),
    }
}

struct CapFrame {
    seq: usize,
    ts_ms: u64,
    dir: Dir,
    bytes: Vec<u8>,
}

/// One direction's deframing state: a Deframer plus the pending-start
/// timestamp, with the same semantics as `pending_since_ms` in session.rs —
/// set when bytes enter an empty deframer, cleared when nothing is pending.
struct Lane {
    dir: Dir,
    deframer: Deframer,
    pending_since: Option<u64>,
    last_ts: Option<u64>,
}

impl Lane {
    fn new(dir: Dir, framing: &Framing) -> Self {
        Self { dir, deframer: Deframer::new(framing.clone()), pending_since: None, last_ts: None }
    }

    /// Idle-gap boundary, driven by record timestamps instead of the wall
    /// clock: flush pending once the gap to `now_ts` reaches idle_ms.
    /// `now_ts: None` is the end of the file — the wire went quiet for good.
    fn idle_flush(&mut self, now_ts: Option<u64>, out: &mut Vec<CapFrame>) {
        let Framing::Idle { idle_ms } = self.deframer.framing() else { return };
        let idle_ms = *idle_ms;
        if self.deframer.pending_len() == 0 {
            return;
        }
        let gap_hit = match (now_ts, self.last_ts) {
            (Some(now), Some(last)) => now.saturating_sub(last) >= idle_ms,
            (None, _) => true,
            (_, None) => false,
        };
        if gap_hit {
            if let Some(bytes) = self.deframer.flush_pending() {
                let ts = self.pending_since.take().unwrap_or_else(|| self.last_ts.unwrap_or(0));
                out.push(CapFrame { seq: 0, ts_ms: ts, dir: self.dir, bytes });
            }
        }
    }

    fn push(&mut self, rec: &ObcapRecord, out: &mut Vec<CapFrame>) {
        self.idle_flush(Some(rec.ts_ms), out);
        if self.pending_since.is_none() {
            self.pending_since = Some(rec.ts_ms);
        }
        for bytes in self.deframer.push(&rec.bytes) {
            out.push(CapFrame { seq: 0, ts_ms: rec.ts_ms, dir: self.dir, bytes });
        }
        if self.deframer.pending_len() == 0 {
            self.pending_since = None;
        }
        self.last_ts = Some(rec.ts_ms);
    }
}

/// Run the capture's records through one deframer per direction. Returns the
/// frames (time-sorted, seq assigned) plus the per-direction byte counts left
/// pending at the end under a non-idle framing.
fn deframe_capture(records: &[ObcapRecord], framing: &Framing) -> (Vec<CapFrame>, (u64, u64)) {
    let mut tx = Lane::new(Dir::Tx, framing);
    let mut rx = Lane::new(Dir::Rx, framing);
    let mut frames = Vec::new();
    for rec in records {
        match rec.dir {
            Dir::Tx => tx.push(rec, &mut frames),
            Dir::Rx => rx.push(rec, &mut frames),
        }
    }
    tx.idle_flush(None, &mut frames);
    rx.idle_flush(None, &mut frames);
    let unframed = (tx.deframer.pending_len() as u64, rx.deframer.pending_len() as u64);
    // An idle flush triggered late can carry an earlier pending-start ts than
    // frames already emitted from the other lane; a stable time sort restores
    // wire order without reordering equal timestamps.
    frames.sort_by_key(|f| f.ts_ms);
    for (i, frame) in frames.iter_mut().enumerate() {
        frame.seq = i;
    }
    (frames, unframed)
}

// ---------------------------------------------------------------------------
// diagnose_frame
// ---------------------------------------------------------------------------

const ALL_CHECKSUMS: [ChecksumKind; 7] = [
    ChecksumKind::Crc16Modbus,
    ChecksumKind::Crc16Ccitt,
    ChecksumKind::Crc8,
    ChecksumKind::Crc32,
    ChecksumKind::Xor8,
    ChecksumKind::Sum8,
    ChecksumKind::Sum16Be,
];

pub fn diagnose_frame(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let bytes = hex::parse_hex(arg_str(args, "hex")?)?;
    if bytes.is_empty() {
        bail!("hex decodes to zero bytes — nothing to diagnose");
    }

    let mut result = json!({
        "frame_len": bytes.len(),
        "hex": hex::to_hex(&bytes),
        "checksum_matrix": checksum_matrix(&bytes),
        "view": { "kind": "diagnostics" },
    });

    if let Some(expected) = args.get("expected").filter(|v| !v.is_null()) {
        let device_name = expected
            .get("device")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("expected.device must be a string"))?;
        let command_name = expected
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("expected.command must be a string"))?;
        let device = ctx.workspace.load_device(device_name)?;
        let cmd = device.command(command_name)?;
        let parse = cmd.response.as_ref().and_then(|r| r.parse.as_ref()).ok_or_else(|| {
            anyhow!(
                "command {device_name}/{command_name} declares no response.parse — \
                 nothing to probe the frame with"
            )
        })?;
        let attempts: Vec<Value> =
            (-2i64..=2).map(|offset| parse_attempt(parse, &bytes, offset)).collect();
        result
            .as_object_mut()
            .expect("result is an object")
            .insert("parse_attempts".to_string(), json!(attempts));
    }
    shape_result(result, &ctx.workspace.root, "diagnose_frame", output::DEFAULT_MAX_INLINE_BYTES)
}

/// Every checksum algorithm x value encoding, verified at the default tail
/// position via the same `verify_checksum` the real parse path uses. A
/// verified row's `at` is the byte offset where the stored checksum starts
/// (frame_len - stored_len); a row whose algorithm cannot apply to this
/// frame carries only kind/encoding/error — no fabricated position, no
/// verdict.
fn checksum_matrix(frame: &[u8]) -> Vec<Value> {
    let mut rows = Vec::new();
    for kind in ALL_CHECKSUMS {
        for (encoding, enc_name) in [(Encoding::Raw, "raw"), (Encoding::AsciiHex, "ascii_hex")] {
            let spec = ValidateSpec {
                checksum: kind.name().to_string(),
                range: None,
                at: None,
                encoding,
            };
            // Bytes the stored checksum occupies at the tail: the algorithm's
            // byte count raw, twice that as ASCII hex characters.
            let stored = match encoding {
                Encoding::Raw => kind.len(),
                Encoding::AsciiHex => 2 * kind.len(),
            };
            let mut row = Map::new();
            row.insert("kind".to_string(), json!(kind.name()));
            row.insert("encoding".to_string(), json!(enc_name));
            match verify_checksum(&spec, frame) {
                Ok(()) => {
                    row.insert("at".to_string(), json!(frame.len() - stored));
                    row.insert("ok".to_string(), json!(true));
                    // The stored value equals the computed one on a hit; show
                    // the computed bytes over the same default range.
                    let computed = kind.compute(&frame[..frame.len() - stored]);
                    row.insert("computed".to_string(), json!(hex::to_hex(&computed)));
                }
                Err(CoreError::ChecksumMismatch { expected, actual, .. }) => {
                    row.insert("at".to_string(), json!(frame.len() - stored));
                    row.insert("ok".to_string(), json!(false));
                    row.insert("expected".to_string(), json!(expected));
                    row.insert("actual".to_string(), json!(actual));
                }
                Err(e) => {
                    // The variant does not apply to this frame at all (too
                    // short, non-ASCII slot, …) — the reason alone; `at`
                    // would be meaningless or negative here.
                    row.insert("error".to_string(), json!(e.to_string()));
                }
            }
            rows.push(Value::Object(row));
        }
    }
    rows
}

fn parse_attempt(parse: &ParseSpec, frame: &[u8], offset: i64) -> Value {
    let mut row = Map::new();
    row.insert("offset".to_string(), json!(offset));
    // The verdict is named `parsed`, not `ok`: structurally decodable at this
    // offset. The offsets are mutually exclusive hypotheses — several may
    // parse, and which one is the real alignment is the caller's judgment.
    match probe_at_offset(parse, frame, offset) {
        Ok(fields) => {
            row.insert("parsed".to_string(), json!(true));
            row.insert("fields".to_string(), fields);
        }
        Err(reason) => {
            row.insert("parsed".to_string(), json!(false));
            row.insert("error".to_string(), json!(reason));
        }
    }
    Value::Object(row)
}

/// Try the parse spec with every top-level byte offset shifted by `offset`
/// (element offsets are record-relative and shift along). A regex parse has
/// no byte offsets: positive shifts drop leading bytes, negative shifts would
/// need bytes before the frame and fail with the reason.
fn probe_at_offset(parse: &ParseSpec, frame: &[u8], offset: i64) -> Result<Value, String> {
    if let Some(fields) = &parse.fields {
        let mut shifted = BTreeMap::new();
        for (name, field) in fields {
            let at = usize::try_from(field.at as i64 + offset).map_err(|_| {
                format!(
                    "offset {offset} puts field {name:?} before byte 0 (declared at {})",
                    field.at
                )
            })?;
            let mut moved = field.clone();
            moved.at = at;
            shifted.insert(name.clone(), moved);
        }
        let spec = ParseSpec { fields: Some(shifted), regex: None, types: None, split: None };
        return parse_with_spec(&spec, frame).map_err(|e| e.to_string());
    }
    if parse.regex.is_some() {
        let start = usize::try_from(offset)
            .map_err(|_| format!("offset {offset}: a regex parse cannot start before the frame"))?;
        if start > frame.len() {
            return Err(format!("offset {offset} is past the {}-byte frame", frame.len()));
        }
        return parse_with_spec(parse, &frame[start..]).map_err(|e| e.to_string());
    }
    Err("parse spec declares neither fields nor regex".to_string())
}

// ---------------------------------------------------------------------------
// session_stats
// ---------------------------------------------------------------------------

pub fn session_stats(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let sessions = match args.get("session_id").and_then(Value::as_str) {
        Some(id) => vec![ctx.sessions.get(id)?],
        None => ctx.sessions.all(),
    };
    let stats: Vec<Value> = sessions.iter().map(|s| s.stats()).collect();
    Ok(json!({ "sessions": stats }))
}

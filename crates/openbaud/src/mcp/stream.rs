//! The `stream_poll` tool: per-consumer incremental frame subscriptions over
//! live sessions. Schema and dispatch only — the subscription engine lives in
//! `engine/stream.rs`. Deliberately a plain data tool with no `_meta.ui`
//! binding (risk ④): host approval/throttling of widget-driven polling is
//! unproven, so no widget is steered at this tool.

use crate::engine::now_ms;
use crate::engine::session::Session;
use crate::engine::stream::{
    StreamParse, DEFAULT_POLL_FRAMES, DEFAULT_POLL_INLINE_BYTES, MAX_POLL_FRAMES,
    MAX_POLL_INLINE_BYTES, MIN_POLL_INLINE_BYTES, SUBSCRIPTION_IDLE_TTL_MS,
};
use crate::mcp::tools::arg_u64_or;
use crate::mcp::Ctx;
use anyhow::{anyhow, bail};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn spec() -> Value {
    json!({
        "name": "stream_poll",
        "description": "Per-consumer incremental frame poll on an open session — a private \
            cursor with its own deframer, fully independent of `read` (the two never steal \
            frames from each other). Call with session_id to create a subscription: the result \
            carries its subscription_id plus any frames already buffered. Call with that \
            subscription_id to fetch what arrived since; frames carry a monotonic \
            per-subscription seq, and next_seq is the seq the next frame will get. since_seq \
            acknowledges delivery: frames with seq < since_seq are released — ack with the \
            last delivered frame's seq + 1, not next_seq (next_seq can run ahead of a \
            max_frames-capped page, and acknowledging frames never delivered is a loud \
            error). Each page also stops at max_inline_bytes of rendered payload (default \
            4096, metered as each frame's hex length + text length; page_bytes reports the \
            page's actual total) — whole frames only, in seq order; a first frame alone \
            beyond the budget is still delivered, alone, with oversized_frame: true, so \
            delivery always makes progress. Omit since_seq and the unacknowledged frames \
            are redelivered unchanged \
            (idempotent re-read, so a lost response costs nothing). A consumer that lags \
            loses the oldest retained frames beyond 1024, counted cumulatively in \
            dropped_frames; a subscription the session's byte buffer overflowed past counts \
            the gaps in dropped_chunks — never silently. close: true releases the \
            subscription (result carries closed: true). At most 8 subscriptions per session; \
            ones idle beyond 120 s are swept by later stream_poll calls, and closing the \
            session releases them all. Every result folds in the session's live counters as \
            stats. Returns immediately with whatever is available — never waits; a poll with \
            nothing left to deliver on a dead port errors loudly instead of looking healthy. \
            Optionally pass parse: {device, command} when CREATING a subscription (never on a \
            follow-up poll — that is a loud error) to parse every frame server-side with that \
            workspace command's response.parse: each frame then carries parsed (the field \
            values) or parse_error (the per-frame reason; one bad frame never stops the \
            stream), parsed once at arrival so redelivery repeats the identical outcome, and \
            every result echoes parse: {device, command} plus the command's units (same \
            semantics as run_command). A device or command that does not exist, or a command \
            without a response.parse block, fails subscription creation loudly.",
        "inputSchema": { "type": "object", "properties": {
            "session_id": { "type": "string", "description": "Open session to subscribe to; required when no subscription_id is given" },
            "subscription_id": { "type": "string", "description": "Subscription from an earlier stream_poll; required for follow-up polls and for close" },
            "since_seq": { "type": "integer", "description": "Acknowledge delivery: release frames with seq < since_seq — pass the last delivered frame's seq + 1; omit to redeliver the unacknowledged frames" },
            "max_frames": { "type": "integer", "default": DEFAULT_POLL_FRAMES, "minimum": 1, "maximum": MAX_POLL_FRAMES },
            "max_inline_bytes": { "type": "integer", "default": DEFAULT_POLL_INLINE_BYTES, "minimum": MIN_POLL_INLINE_BYTES, "maximum": MAX_POLL_INLINE_BYTES, "description": "Byte budget for one page's inlined frames, metered as each frame's rendered hex length + text length; whole frames only, except a single first frame beyond the budget (delivered with oversized_frame: true)" },
            "parse": { "type": "object", "properties": { "device": { "type": "string" }, "command": { "type": "string" } }, "required": ["device", "command"], "description": "Only when creating a subscription (with session_id): parse every frame server-side with this workspace command's response.parse — frames carry parsed or parse_error; a follow-up poll passing parse is a loud error" },
            "close": { "type": "boolean", "description": "Release the subscription instead of polling it" }
        }},
        "annotations": {
            "readOnlyHint": true,
            "openWorldHint": true,
            "destructiveHint": false
        },
    })
}

pub fn stream_poll(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let now = now_ms();
    let session_id = args.get("session_id").and_then(Value::as_str);
    let subscription_id = args.get("subscription_id").and_then(Value::as_str);
    let close = args.get("close").and_then(Value::as_bool).unwrap_or(false);
    let since_seq = match args.get("since_seq") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .ok_or_else(|| anyhow!("since_seq must be a non-negative integer, got {v}"))?,
        ),
    };
    // Range-check before narrowing so an out-of-range u64 is rejected as the
    // value the caller sent, not whatever it truncates to on this target.
    let max_frames = arg_u64_or(args, "max_frames", DEFAULT_POLL_FRAMES as u64)?;
    if max_frames == 0 || max_frames > MAX_POLL_FRAMES as u64 {
        bail!("max_frames must be in 1..={MAX_POLL_FRAMES}, got {max_frames}");
    }
    let max_frames = max_frames as usize;
    // Same range-check-before-narrowing rule as max_frames above.
    let max_inline_bytes = arg_u64_or(args, "max_inline_bytes", DEFAULT_POLL_INLINE_BYTES as u64)?;
    if max_inline_bytes < MIN_POLL_INLINE_BYTES as u64
        || max_inline_bytes > MAX_POLL_INLINE_BYTES as u64
    {
        bail!(
            "max_inline_bytes must be in {MIN_POLL_INLINE_BYTES}..={MAX_POLL_INLINE_BYTES}, \
             got {max_inline_bytes}"
        );
    }
    let max_inline_bytes = max_inline_bytes as usize;
    if close && since_seq.is_some() {
        bail!(
            "close: true does not take since_seq — closing releases every retained frame \
             regardless"
        );
    }

    match (subscription_id, session_id) {
        (Some(sub_id), sid) => {
            // parse is a creation-time property of the subscription — a
            // follow-up poll trying to set (or repeat) it is a loud error,
            // never silently ignored.
            if args.get("parse").is_some_and(|v| !v.is_null()) {
                bail!(
                    "parse is set when the subscription is created (the call with \
                     session_id only) — a follow-up poll cannot change or repeat it; \
                     to parse with a different command, close this subscription and \
                     create a new one"
                );
            }
            let session = find_subscription_session(sub_id, ctx)?;
            if let Some(sid) = sid {
                if sid != session.id {
                    bail!(
                        "subscription {sub_id:?} belongs to session {:?}, not {sid:?}",
                        session.id
                    );
                }
            }
            if close {
                return session.stream_close(sub_id, now);
            }
            session.stream_pull(sub_id, since_seq, max_frames, max_inline_bytes, now)
        }
        (None, Some(sid)) => {
            if close {
                bail!("close: true requires subscription_id — there is no subscription to close yet");
            }
            if since_seq.is_some() {
                bail!(
                    "since_seq requires subscription_id — a new subscription has delivered \
                     nothing to acknowledge"
                );
            }
            // Resolve parse from the workspace before subscribing, so a bad
            // device/command/parse block fails loudly without consuming a
            // subscription slot.
            let parse = stream_parse_arg(args, ctx)?;
            let session = ctx.sessions.get(sid)?;
            let sub_id = session.stream_subscribe(now, parse)?;
            session.stream_pull(&sub_id, None, max_frames, max_inline_bytes, now)
        }
        (None, None) => bail!(
            "stream_poll needs session_id (to create a subscription) or subscription_id \
             (to poll an existing one)"
        ),
    }
}

/// Resolve the optional `parse: {device, command}` creation argument against
/// the workspace: the named command's `response.parse` becomes the
/// subscription's per-frame parse spec, its field units travel along (same
/// semantics as `run_command`). Every failure — malformed argument, unknown
/// device or command, command without a parse block — is loud.
fn stream_parse_arg(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Option<StreamParse>> {
    let Some(v) = args.get("parse").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("parse must be an object {{device, command}}, got {v}"))?;
    let field = |key: &str| -> anyhow::Result<&str> {
        obj.get(key).and_then(Value::as_str).ok_or_else(|| {
            anyhow!("parse.{key} must be a string — parse is {{device, command}} naming a workspace command")
        })
    };
    let device_name = field("device")?;
    let command_name = field("command")?;
    let device = ctx.workspace.load_device(device_name)?;
    let cmd = device.command(command_name)?;
    let Some(spec) = cmd.response.as_ref().and_then(|r| r.parse.as_ref()) else {
        bail!(
            "command {command_name:?} on device {device_name:?} declares no response.parse \
             block — stream_poll can only parse frames with a command that declares one"
        );
    };
    Ok(Some(StreamParse {
        device: device_name.to_string(),
        command: command_name.to_string(),
        spec: spec.clone(),
        units: openbaud_core::exec::units(cmd),
    }))
}

/// The session holding a subscription id. Ids embed their session id, but the
/// lookup goes through the live sessions so a stale id is a loud, explained
/// error instead of a dangling reference.
fn find_subscription_session(sub_id: &str, ctx: &Arc<Ctx>) -> anyhow::Result<Arc<Session>> {
    ctx.sessions.all().into_iter().find(|s| s.has_stream_subscription(sub_id)).ok_or_else(|| {
        anyhow!(
            "no stream subscription {sub_id:?} on any open session — it was closed, expired \
             (idle beyond {SUBSCRIPTION_IDLE_TTL_MS} ms), or its session is gone; subscribe \
             again with session_id"
        )
    })
}

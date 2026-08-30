//! Minimal MCP server over stdio: newline-delimited JSON-RPC 2.0 implementing
//! initialize / ping / tools/list / tools/call. Hand-rolled on purpose — four
//! methods, zero protocol-SDK drift.

mod capture;
mod data;
pub mod resources;
mod stream;
pub mod tools;

use crate::engine::audit::Audit;
use crate::engine::session::SessionManager;
use crate::workspace::Workspace;
use anyhow::Context;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct Ctx {
    pub sessions: SessionManager,
    pub workspace: Workspace,
    pub audit: Audit,
    /// Client identity from `initialize` (name/version), kept for host-capability
    /// routing (e.g. Apps confirm-card vs elicitation). Self-reported, unverifiable.
    pub client_info: std::sync::OnceLock<Value>,
}

const PROTOCOL_FALLBACK: &str = "2025-06-18";

pub async fn serve(ctx: Arc<Ctx>) -> anyhow::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await.context("stdin read failed")? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(&mut stdout, &rpc_error(Value::Null, -32700, &format!("parse error: {e}")))
                    .await?;
                continue;
            }
        };
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            continue; // a response or malformed message; nothing to do
        };
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        if id.is_none() {
            // Notification (e.g. notifications/initialized): no response.
            continue;
        }
        let id = id.expect("checked above");

        let response = match handle(method, params, &ctx).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(RpcError { code, message }) => rpc_error(id, code, &message),
        };
        write_msg(&mut stdout, &response).await?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// Handle one JSON-RPC request. Public as the protocol-layer test seam:
/// integration tests drive this directly instead of going through stdio.
pub async fn handle(method: &str, params: Value, ctx: &Arc<Ctx>) -> Result<Value, RpcError> {
    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_FALLBACK);
            if let Some(client) = params.get("clientInfo") {
                let _ = ctx.client_info.set(client.clone());
            }
            Ok(json!({
                "protocolVersion": requested,
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "listChanged": false },
                },
                "serverInfo": {
                    "name": "openbaud",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": tools::SERVER_INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::list() })),
        "resources/list" => Ok(resources::list()),
        "resources/templates/list" => Ok(resources::templates()),
        "resources/read" => {
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError { code: -32602, message: "resources/read requires uri".into() })?;
            resources::read(uri, &ctx.workspace.root)
                .map_err(|e| RpcError { code: -32002, message: format!("{e:#}") })?
                .ok_or_else(|| RpcError { code: -32002, message: format!("unknown resource {uri:?}") })
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError { code: -32602, message: "tools/call requires name".into() })?
                .to_string();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match tools::call(&name, args, ctx).await {
                Ok(result) => {
                    let text = serde_json::to_string_pretty(&result).expect("tool results are valid JSON");
                    // Same shaped (summarized) value in both channels: the model reads
                    // the text block, an Apps widget consumes structuredContent.
                    Ok(json!({
                        "content": [{ "type": "text", "text": text }],
                        "structuredContent": result,
                    }))
                }
                Err(e) => Ok(json!({
                    "content": [{ "type": "text", "text": format!("error: {e:#}") }],
                    "isError": true,
                })),
            }
        }
        other => Err(RpcError { code: -32601, message: format!("method {other:?} not supported") }),
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn write_msg(stdout: &mut tokio::io::Stdout, msg: &Value) -> anyhow::Result<()> {
    let mut line = serde_json::to_vec(msg).context("cannot serialize response")?;
    line.push(b'\n');
    stdout.write_all(&line).await.context("stdout write failed")?;
    stdout.flush().await.context("stdout flush failed")?;
    Ok(())
}

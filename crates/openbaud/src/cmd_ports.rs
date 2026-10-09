//! `openbaud ports` — list serial ports, each with the workspace devices whose
//! profile selector matches it and the canonical port a /dev/tty.* twin
//! mirrors: the same enrichment the MCP `list_ports` tool reports.

use openbaud::engine::transport;
use openbaud::workspace::Workspace;
use std::path::Path;

pub fn run(workspace_dir: &Path) -> anyhow::Result<()> {
    let workspace = Workspace::at(workspace_dir);
    let mut devices = Vec::new();
    for name in workspace.list_devices() {
        match workspace.load_device(&name) {
            Ok(device) => devices.push((name, device.profile.selector.clone())),
            // A device that fails to load cannot be matched — say so rather
            // than silently listing every port as unmatched.
            Err(e) => eprintln!("warning: device {name:?} not matched: {e:#}"),
        }
    }
    for p in transport::enrich_ports(transport::list_ports()?, &devices, &[]) {
        let mut extras: Vec<String> = Vec::new();
        if let (Some(vid), Some(pid)) = (&p.port.vid, &p.port.pid) {
            extras.push(format!("{vid}:{pid}"));
        }
        if let Some(m) = &p.port.manufacturer {
            extras.push(m.clone());
        }
        if let Some(prod) = &p.port.product {
            extras.push(prod.clone());
        }
        if !p.matches_devices.is_empty() {
            extras.push(format!("matches {}", p.matches_devices.join(", ")));
        }
        if let Some(cu) = &p.alias_of {
            extras.push(format!("same device as {cu}"));
        }
        if extras.is_empty() {
            println!("{:<28} {}", p.port.path, p.port.kind);
        } else {
            println!("{:<28} {}  {}", p.port.path, p.port.kind, extras.join("  "));
        }
    }
    Ok(())
}

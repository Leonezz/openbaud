//! `openbaud ports` — list serial ports.

use openbaud::engine::transport;

pub fn run() -> anyhow::Result<()> {
    let ports = transport::list_ports()?;
    for p in ports {
        let mut extras: Vec<String> = Vec::new();
        if let (Some(vid), Some(pid)) = (&p.vid, &p.pid) {
            extras.push(format!("{vid}:{pid}"));
        }
        if let Some(m) = &p.manufacturer {
            extras.push(m.clone());
        }
        if let Some(prod) = &p.product {
            extras.push(prod.clone());
        }
        if extras.is_empty() {
            println!("{:<28} {}", p.path, p.kind);
        } else {
            println!("{:<28} {}  {}", p.path, p.kind, extras.join("  "));
        }
    }
    Ok(())
}

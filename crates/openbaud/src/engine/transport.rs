//! Port opening: real serial ports via tokio-serial, `mock:echo` — an
//! explicit, always-available loopback for smoke-testing without hardware —
//! and `replay:<path>` for playing back recorded captures. Also home of the
//! profile selector → concrete port resolution.

use anyhow::{anyhow, bail, Context};
use openbaud_core::format::{Parity, SelectorSpec, Transport as TransportCfg};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_serial::SerialPortBuilderExt;

pub trait Port: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Port for T {}

pub type BoxedPort = Box<dyn Port>;

pub const MOCK_ECHO: &str = "mock:echo";
/// Port name prefix for capture replay: `replay:<path-to-.obcap>`.
pub const REPLAY_PREFIX: &str = "replay:";

/// Interval between retries when the port reports "busy" on open.
const BUSY_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
/// Total time budget for busy retries. On macOS a closed tty takes ~0.5s to
/// tear down, so a close-then-reopen sequence transiently fails with EBUSY.
const BUSY_RETRY_BUDGET: std::time::Duration = std::time::Duration::from_millis(1500);

/// True for open failures that mean "someone (possibly the kernel, mid-teardown)
/// still holds the device" — the only class of error worth retrying.
fn is_busy_error(err: &tokio_serial::Error) -> bool {
    let desc = err.to_string().to_lowercase();
    desc.contains("busy") || desc.contains("os error 16")
}

pub async fn open_port(name: &str, cfg: &TransportCfg) -> anyhow::Result<BoxedPort> {
    if let Some(path) = name.strip_prefix(REPLAY_PREFIX) {
        return crate::engine::replay::open_replay(std::path::Path::new(path));
    }
    if name == MOCK_ECHO {
        let (client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let (mut read, mut write) = tokio::io::split(server);
            let _ = tokio::io::copy(&mut read, &mut write).await;
        });
        return Ok(Box::new(client));
    }
    if name.starts_with("mock:") {
        bail!("unknown mock port {name:?} — only {MOCK_ECHO:?} is available");
    }

    let data_bits = match cfg.data_bits {
        5 => tokio_serial::DataBits::Five,
        6 => tokio_serial::DataBits::Six,
        7 => tokio_serial::DataBits::Seven,
        8 => tokio_serial::DataBits::Eight,
        other => bail!("unsupported data_bits {other} (expected 5..=8)"),
    };
    let parity = match cfg.parity {
        Parity::None => tokio_serial::Parity::None,
        Parity::Even => tokio_serial::Parity::Even,
        Parity::Odd => tokio_serial::Parity::Odd,
    };
    let stop_bits = match cfg.stop_bits {
        1 => tokio_serial::StopBits::One,
        2 => tokio_serial::StopBits::Two,
        other => bail!("unsupported stop_bits {other} (expected 1 or 2)"),
    };

    let builder = tokio_serial::new(name, cfg.baud)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits);
    let deadline = tokio::time::Instant::now() + BUSY_RETRY_BUDGET;
    let mut retries: u32 = 0;
    loop {
        match builder.clone().open_native_async() {
            Ok(stream) => return Ok(Box::new(stream)),
            Err(e) if is_busy_error(&e) && tokio::time::Instant::now() < deadline => {
                retries += 1;
                tokio::time::sleep(BUSY_RETRY_INTERVAL).await;
            }
            Err(e) => {
                let retried = if retries > 0 {
                    format!(
                        " after {retries} retries over {:.1}s — is another process holding the port?",
                        BUSY_RETRY_BUDGET.as_secs_f64()
                    )
                } else {
                    String::new()
                };
                return Err(e).with_context(|| {
                    format!("cannot open serial port {name:?} at {} baud{retried}", cfg.baud)
                });
            }
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct PortInfo {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
}

impl PortInfo {
    pub fn usb(path: &str, vid: &str, pid: &str, manufacturer: Option<&str>) -> Self {
        PortInfo {
            path: path.to_string(),
            kind: "usb",
            vid: Some(vid.to_string()),
            pid: Some(pid.to_string()),
            manufacturer: manufacturer.map(str::to_string),
            product: None,
            serial_number: None,
        }
    }

    pub fn mock() -> Self {
        PortInfo {
            path: MOCK_ECHO.to_string(),
            kind: "mock",
            vid: None,
            pid: None,
            manufacturer: None,
            product: Some("loopback echo (always available, no hardware needed)".to_string()),
            serial_number: None,
        }
    }
}

/// A port plus what the workspace knows about it: which device profiles claim
/// it, whether a session already holds it, and — on macOS — which `/dev/cu.*`
/// entry a `/dev/tty.*` twin duplicates.
#[derive(Debug, serde::Serialize)]
pub struct EnrichedPort {
    #[serde(flatten)]
    pub port: PortInfo,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matches_devices: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<String>,
}

/// Pure enrichment over an enumerated port list — the testable core of what
/// `list_ports` and `ask_port` return.
pub fn enrich_ports(
    ports: Vec<PortInfo>,
    devices: &[(String, Option<SelectorSpec>)],
    open_sessions: &[(String, String)],
) -> Vec<EnrichedPort> {
    let canonical: Vec<&str> = ports.iter().map(|p| p.path.as_str()).collect();
    let alias_target = |path: &str| -> Option<String> {
        let suffix = path.strip_prefix("/dev/tty.")?;
        let cu = format!("/dev/cu.{suffix}");
        canonical.contains(&cu.as_str()).then_some(cu)
    };

    ports
        .iter()
        .map(|port| EnrichedPort {
            matches_devices: devices
                .iter()
                .filter(|(_, sel)| {
                    sel.as_ref().is_some_and(|s| port.kind != "mock" && selector_matches(port, s))
                })
                .map(|(name, _)| name.clone())
                .collect(),
            open_session: open_sessions
                .iter()
                .find(|(port_name, _)| *port_name == port.path)
                .map(|(_, id)| id.clone()),
            alias_of: alias_target(&port.path),
            port: PortInfo { ..clone_port(port) },
        })
        .collect()
}

fn clone_port(p: &PortInfo) -> PortInfo {
    PortInfo {
        path: p.path.clone(),
        kind: p.kind,
        vid: p.vid.clone(),
        pid: p.pid.clone(),
        manufacturer: p.manufacturer.clone(),
        product: p.product.clone(),
        serial_number: p.serial_number.clone(),
    }
}

pub fn list_ports() -> anyhow::Result<Vec<PortInfo>> {
    let mut out: Vec<PortInfo> = tokio_serial::available_ports()
        .map_err(|e| anyhow!("cannot enumerate serial ports: {e}"))?
        .into_iter()
        .map(|p| match p.port_type {
            tokio_serial::SerialPortType::UsbPort(usb) => PortInfo {
                path: p.port_name,
                kind: "usb",
                vid: Some(format!("{:04X}", usb.vid)),
                pid: Some(format!("{:04X}", usb.pid)),
                manufacturer: usb.manufacturer,
                product: usb.product,
                serial_number: usb.serial_number,
            },
            tokio_serial::SerialPortType::BluetoothPort => PortInfo {
                path: p.port_name,
                kind: "bluetooth",
                vid: None,
                pid: None,
                manufacturer: None,
                product: None,
                serial_number: None,
            },
            _ => PortInfo {
                path: p.port_name,
                kind: "native",
                vid: None,
                pid: None,
                manufacturer: None,
                product: None,
                serial_number: None,
            },
        })
        .collect();
    out.push(PortInfo {
        path: MOCK_ECHO.to_string(),
        kind: "mock",
        vid: None,
        pid: None,
        manufacturer: None,
        product: Some("loopback echo (always available, no hardware needed)".to_string()),
        serial_number: None,
    });
    Ok(out)
}

// ---------------------------------------------------------------------------
// Device selector → concrete port
// ---------------------------------------------------------------------------

/// Resolve a profile selector against the live port list. Exactly one match
/// is required; zero or several is a loud error — never a silent pick.
pub fn resolve_selector(
    selector: &SelectorSpec,
    device_name: &str,
) -> anyhow::Result<String> {
    select_port(&list_ports()?, selector, device_name)
}

/// Pure selector matching over an enumerated port list (the testable core of
/// `resolve_selector`). All present selector fields must match (AND); mock
/// entries never match; on macOS a physical port exposed as both `/dev/cu.X`
/// and `/dev/tty.X` is deduplicated in favor of `cu.`.
pub fn select_port(
    ports: &[PortInfo],
    selector: &SelectorSpec,
    device_name: &str,
) -> anyhow::Result<String> {
    let hits: Vec<&PortInfo> = ports
        .iter()
        .filter(|p| p.kind != "mock" && selector_matches(p, selector))
        .collect();

    let mut paths: Vec<&str> = hits.iter().map(|p| p.path.as_str()).collect();
    paths.retain(|path| match path.strip_prefix("/dev/tty.") {
        Some(suffix) => !hits.iter().any(|h| h.path == format!("/dev/cu.{suffix}")),
        None => true,
    });

    match paths.as_slice() {
        [one] => Ok((*one).to_string()),
        [] => bail!(
            "no port matches the selector for device {device_name:?} ({}); available ports: {}",
            describe_selector(selector),
            describe_candidates(ports),
        ),
        many => bail!(
            "selector for device {device_name:?} matches {} ports: {} — specify the port explicitly",
            many.len(),
            many.join(", "),
        ),
    }
}

fn selector_matches(port: &PortInfo, sel: &SelectorSpec) -> bool {
    // vid/pid: hex value comparison — case-insensitive and padding-agnostic.
    let hex_matches = |want: &Option<String>, have: &Option<String>| match want {
        None => true,
        Some(w) => match (u16::from_str_radix(w, 16), have) {
            (Ok(w), Some(h)) => u16::from_str_radix(h, 16).is_ok_and(|h| h == w),
            _ => false,
        },
    };
    let serial_ok = match &sel.serial_number {
        None => true,
        Some(s) => port.serial_number.as_deref() == Some(s.as_str()),
    };
    let product_ok = match &sel.product {
        None => true,
        Some(p) => port.product.as_deref().is_some_and(|prod| prod.contains(p.as_str())),
    };
    hex_matches(&sel.vid, &port.vid) && hex_matches(&sel.pid, &port.pid) && serial_ok && product_ok
}

fn describe_selector(sel: &SelectorSpec) -> String {
    let mut parts = Vec::new();
    if let Some(v) = &sel.vid {
        parts.push(format!("vid={v}"));
    }
    if let Some(p) = &sel.pid {
        parts.push(format!("pid={p}"));
    }
    if let Some(s) = &sel.serial_number {
        parts.push(format!("serial_number={s}"));
    }
    if let Some(p) = &sel.product {
        parts.push(format!("product contains {p:?}"));
    }
    parts.join(", ")
}

fn describe_candidates(ports: &[PortInfo]) -> String {
    let described: Vec<String> = ports
        .iter()
        .filter(|p| p.kind != "mock")
        .map(|p| {
            let mut attrs = Vec::new();
            if let Some(v) = &p.vid {
                attrs.push(format!("vid={v}"));
            }
            if let Some(pid) = &p.pid {
                attrs.push(format!("pid={pid}"));
            }
            if let Some(prod) = &p.product {
                attrs.push(format!("product={prod:?}"));
            }
            if let Some(s) = &p.serial_number {
                attrs.push(format!("serial_number={s}"));
            }
            if attrs.is_empty() {
                p.path.clone()
            } else {
                format!("{} ({})", p.path, attrs.join(", "))
            }
        })
        .collect();
    if described.is_empty() {
        "none".to_string()
    } else {
        described.join("; ")
    }
}

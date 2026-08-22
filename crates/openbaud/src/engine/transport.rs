//! Port opening: real serial ports via tokio-serial, plus `mock:echo` — an
//! explicit, always-available loopback for smoke-testing without hardware.

use anyhow::{anyhow, bail, Context};
use openbaud_core::format::{Parity, Transport as TransportCfg};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_serial::SerialPortBuilderExt;

pub trait Port: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Port for T {}

pub type BoxedPort = Box<dyn Port>;

pub const MOCK_ECHO: &str = "mock:echo";

pub fn open_port(name: &str, cfg: &TransportCfg) -> anyhow::Result<BoxedPort> {
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

    let stream = tokio_serial::new(name, cfg.baud)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .open_native_async()
        .with_context(|| format!("cannot open serial port {name:?} at {} baud", cfg.baud))?;
    Ok(Box::new(stream))
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

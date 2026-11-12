pub mod info;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use crate::errors::QamError;

pub const DEFAULT_WSD_ADDR: &str = "239.255.255.250:3702";

pub const PROBE_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <e:Header>
    <w:MessageID>uuid:%s</w:MessageID>
    <w:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To>
    <w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>
  </e:Header>
  <e:Body>
    <d:Probe>
      <d:Types>dn:NetworkVideoTransmitter</d:Types>
    </d:Probe>
  </e:Body>
</e:Envelope>"#;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    pub address: String,
    pub host: String,
    pub model: String,
    pub fw: String,
}

/// Extracts the raw authority (host[:port]) from a URL, matching Go's `u.Host` behavior.
/// Keeps explicit ports (even :80) and drops userinfo.
pub fn host_port(addr: &str) -> String {
    let s = addr.trim();
    let proto_idx = match s.find("://") {
        Some(idx) => idx + 3,
        None => return String::new(),
    };

    let rest = &s[proto_idx..];
    let end_idx = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end_idx];

    let host_part = if let Some(at_idx) = authority.rfind('@') {
        &authority[at_idx + 1..]
    } else {
        authority
    };

    host_part.to_string()
}

/// Parses WS-Discovery ProbeMatch response XML and returns all discovered XAddrs.
pub fn parse_probe_match(data: &[u8]) -> Result<Vec<String>, QamError> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);

    let mut addrs = Vec::new();
    let mut in_xaddrs = false;
    let mut saw_envelope = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                if local.as_ref().eq_ignore_ascii_case(b"Envelope") {
                    saw_envelope = true;
                } else if local.as_ref().eq_ignore_ascii_case(b"XAddrs") {
                    in_xaddrs = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_xaddrs {
                    let text = e.unescape().map_err(|err| {
                        QamError::Discovery(format!("XML text unescape error: {err}"))
                    })?;
                    for word in text.split_whitespace() {
                        addrs.push(word.to_string());
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"XAddrs") {
                    in_xaddrs = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(QamError::Discovery(format!("XML parse error: {e}"))),
            _ => {}
        }
    }

    if !saw_envelope || addrs.is_empty() {
        return Err(QamError::Discovery("no addresses".to_string()));
    }

    Ok(addrs)
}

/// Deduplicates devices by host (or address if host is empty), keeping first-seen order.
pub fn unique_devices(devs: Vec<Device>) -> Vec<Device> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for d in devs {
        let key = if !d.host.is_empty() {
            d.host.clone()
        } else {
            d.address.clone()
        };
        if seen.insert(key) {
            out.push(d);
        }
    }

    out
}

/// Discovers ONVIF cameras via WS-Discovery probe.
/// If `target_addr` is None, sends to default multicast address `239.255.255.250:3702`.
pub async fn discover(
    timeout: Duration,
    target_addr: Option<&str>,
) -> Result<Vec<Device>, QamError> {
    let timeout = if timeout.is_zero() {
        Duration::from_secs(3)
    } else {
        timeout
    };

    let dest_str = target_addr.unwrap_or(DEFAULT_WSD_ADDR);
    let dest: SocketAddr = dest_str.parse().map_err(|e| {
        QamError::Discovery(format!(
            "invalid discovery target address {dest_str:?}: {e}"
        ))
    })?;

    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| QamError::Discovery(format!("listen udp: {e}")))?;

    let msg_id = format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        rand::random::<u32>(),
        rand::random::<u16>(),
        rand::random::<u16>(),
        rand::random::<u16>(),
        rand::random::<u64>() & 0xffffffffffff,
    );
    let probe_xml = PROBE_TEMPLATE.replace("%s", &msg_id);

    socket
        .send_to(probe_xml.as_bytes(), dest)
        .await
        .map_err(|e| QamError::Discovery(format!("send probe: {e}")))?;

    let mut devices = Vec::new();
    let mut buf = vec![0u8; 8192];

    let start = tokio::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, _src))) => {
                if let Ok(addrs) = parse_probe_match(&buf[..n]) {
                    for addr in addrs {
                        let hp = host_port(&addr);
                        devices.push(Device {
                            address: addr,
                            host: hp,
                            model: String::new(),
                            fw: String::new(),
                        });
                    }
                }
            }
            Ok(Err(e)) => {
                return Err(QamError::Discovery(format!("read udp: {e}")));
            }
            Err(_) => {
                // Timeout elapsed
                break;
            }
        }
    }

    Ok(unique_devices(devices))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROBE_MATCH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <e:Body>
    <d:ProbeMatches>
      <d:ProbeMatch>
        <d:XAddrs>http://192.168.1.50:2020/onvif/device_service</d:XAddrs>
      </d:ProbeMatch>
      <d:ProbeMatch>
        <d:XAddrs>http://192.168.1.51:2020/onvif/device_service</d:XAddrs>
      </d:ProbeMatch>
    </d:ProbeMatches>
  </e:Body>
</e:Envelope>"#;

    #[test]
    fn test_parse_probe_match() {
        let addrs = parse_probe_match(SAMPLE_PROBE_MATCH.as_bytes()).unwrap();
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0], "http://192.168.1.50:2020/onvif/device_service");
        assert_eq!(addrs[1], "http://192.168.1.51:2020/onvif/device_service");
    }

    #[test]
    fn test_unique_devices() {
        let devs = vec![
            Device {
                address: "http://a".into(),
                host: "a".into(),
                ..Default::default()
            },
            Device {
                address: "http://b".into(),
                host: "b".into(),
                ..Default::default()
            },
            Device {
                address: "http://a".into(),
                host: "a".into(),
                ..Default::default()
            },
        ];
        let out = unique_devices(devs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].host, "a");
        assert_eq!(out[1].host, "b");
    }

    #[test]
    fn test_host_port() {
        assert_eq!(host_port("http://10.0.0.1:2020/onvif"), "10.0.0.1:2020");
        assert_eq!(host_port("http://10.0.0.1:80/onvif"), "10.0.0.1:80");
        assert_eq!(
            host_port("http://user:pass@10.0.0.1:80/onvif"),
            "10.0.0.1:80"
        );
        assert_eq!(host_port("nonsense"), "");
    }

    #[test]
    fn test_parse_probe_match_error() {
        assert!(parse_probe_match(b"garbage").is_err());
        assert!(parse_probe_match(b"<e:Envelope></e:Envelope>").is_err());
    }

    #[tokio::test]
    async fn test_discover_timeout() {
        let start = std::time::Instant::now();
        let res = discover(Duration::from_millis(10), Some("127.0.0.1:19999")).await;
        assert!(res.is_ok());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

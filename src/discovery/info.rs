use base64::prelude::*;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use sha1::{Digest, Sha1};
use std::time::Duration;

use crate::errors::QamError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub serial: String,
    pub hardware_id: String,
}

const SOAP_BODY_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Header>%HEADER%</s:Header>
  <s:Body>
    <tds:GetDeviceInformation xmlns:tds="http://www.onvif.org/ver10/device/wsdl"/>
  </s:Body>
</s:Envelope>"#;

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn ws_security_header(
    user: &str,
    pass: &str,
    nonce_override: Option<&[u8]>,
    created_override: Option<&str>,
) -> String {
    let nonce: [u8; 16] = if let Some(n) = nonce_override {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&n[..16]);
        arr
    } else {
        rand::random::<[u8; 16]>()
    };

    let nonce_b64 = BASE64_STANDARD.encode(nonce);

    let created = if let Some(c) = created_override {
        c.to_string()
    } else {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
    };

    let mut hasher = Sha1::new();
    hasher.update(nonce);
    hasher.update(created.as_bytes());
    hasher.update(pass.as_bytes());
    let digest = hasher.finalize();
    let password_digest = BASE64_STANDARD.encode(digest);

    format!(
        "\n<wsse:Security xmlns:wsse=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd\"\n               \
         xmlns:wsu=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd\">\n  \
         <wsse:UsernameToken>\n    \
         <wsse:Username>{}</wsse:Username>\n    \
         <wsse:Password Type=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest\">{}</wsse:Password>\n    \
         <wsse:Nonce>{}</wsse:Nonce>\n    \
         <wsu:Created>{}</wsu:Created>\n  \
         </wsse:UsernameToken>\n\
         </wsse:Security>",
        escape_xml(user),
        password_digest,
        nonce_b64,
        created
    )
}

pub fn basic_auth(user: &str, pass: &str) -> String {
    if user.is_empty() {
        return String::new();
    }
    format!("Basic {}", BASE64_STANDARD.encode(format!("{user}:{pass}")))
}

pub async fn fetch_device_info(
    client: &reqwest::Client,
    xaddr: &str,
    user: &str,
    pass: &str,
) -> Result<DeviceInfo, QamError> {
    if xaddr.trim().is_empty() {
        return Err(QamError::Discovery("xaddr required".to_string()));
    }

    // Attempt WS-Security when user is present
    if !user.is_empty() {
        let wsse = ws_security_header(user, pass, None, None);
        let body = SOAP_BODY_TEMPLATE.replace("%HEADER%", &wsse);
        match do_info_request(client, xaddr, body, "").await {
            Ok(info) => return Ok(info),
            Err(e) => {
                if !e.to_string().contains("auth failed") {
                    return Err(e);
                }
                // Fall through to Basic or anonymous
            }
        }
    }

    // Basic or anonymous fallback
    let body = SOAP_BODY_TEMPLATE.replace("%HEADER%", "");
    let auth = basic_auth(user, pass);
    do_info_request(client, xaddr, body, &auth).await
}

async fn do_info_request(
    client: &reqwest::Client,
    url: &str,
    body: String,
    auth_header: &str,
) -> Result<DeviceInfo, QamError> {
    let mut req = client
        .post(url)
        .header("Content-Type", "application/soap+xml; charset=utf-8")
        .timeout(Duration::from_secs(5))
        .body(body);

    if !auth_header.is_empty() {
        req = req.header("Authorization", auth_header);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| QamError::Discovery(format!("network error: {e}")))?;

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(QamError::Discovery(format!(
            "auth failed (status {})",
            status.as_u16()
        )));
    }

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let trimmed: String = text.chars().take(256).collect();
        return Err(QamError::Discovery(format!(
            "device info status {}: {}",
            status.as_u16(),
            trimmed.trim()
        )));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| QamError::Discovery(format!("read body: {e}")))?;

    parse_device_information(&text)
}

pub fn parse_device_information(xml: &str) -> Result<DeviceInfo, QamError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut info = DeviceInfo::default();
    let mut current_tag = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                current_tag = String::from_utf8_lossy(local.as_ref()).to_string();
            }
            Ok(Event::Text(ref e)) => {
                let text = e
                    .unescape()
                    .map_err(|err| QamError::Discovery(format!("XML unescape error: {err}")))?
                    .trim()
                    .to_string();

                match current_tag.as_str() {
                    "Manufacturer" => info.manufacturer = text,
                    "Model" => info.model = text,
                    "FirmwareVersion" => info.firmware = text,
                    "SerialNumber" => info.serial = text,
                    "HardwareId" | "HardwareID" => info.hardware_id = text,
                    _ => {}
                }
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(QamError::Discovery(format!("XML parse error: {e}"))),
            _ => {}
        }
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_security_header_vector() {
        let nonce = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ];
        let created = "2026-11-12T12:00:00Z";
        let header = ws_security_header("admin&co", "secret123", Some(&nonce), Some(created));

        assert!(header.contains("<wsse:Username>admin&amp;co</wsse:Username>"));
        assert!(header.contains("<wsu:Created>2026-11-12T12:00:00Z</wsu:Created>"));
        assert!(header.contains("<wsse:Nonce>AQIDBAUGBwgJCgsMDQ4PEA==</wsse:Nonce>"));

        // Verify SHA1 digest: sha1(nonce || created || pass)
        let mut hasher = Sha1::new();
        hasher.update(nonce);
        hasher.update(created.as_bytes());
        hasher.update(b"secret123");
        let want_digest = BASE64_STANDARD.encode(hasher.finalize());
        assert!(header.contains(&want_digest));
    }

    #[test]
    fn test_parse_device_information() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <tds:GetDeviceInformationResponse xmlns:tds="http://www.onvif.org/ver10/device/wsdl">
      <tds:Manufacturer> Tapo </tds:Manufacturer>
      <tds:Model> C200 </tds:Model>
      <tds:FirmwareVersion> 1.3.0 </tds:FirmwareVersion>
      <tds:SerialNumber> 12345ABC </tds:SerialNumber>
      <tds:HardwareId> HW1.0 </tds:HardwareId>
    </tds:GetDeviceInformationResponse>
  </s:Body>
</s:Envelope>"#;

        let info = parse_device_information(xml).unwrap();
        assert_eq!(info.manufacturer, "Tapo");
        assert_eq!(info.model, "C200");
        assert_eq!(info.firmware, "1.3.0");
        assert_eq!(info.serial, "12345ABC");
        assert_eq!(info.hardware_id, "HW1.0");
    }
}

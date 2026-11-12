use assert_cmd::Command;
use predicates::prelude::*;
use std::time::Duration;
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

use qam::discovery::discover;
use qam::discovery::info::fetch_device_info;

#[test]
fn test_discover_no_devices_cli() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.args(["discover", "--timeout", "10ms"]);
    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "No devices found. Ensure cameras and this host are on the same LAN.",
        ));
}

#[tokio::test]
async fn test_info_fallback_from_wsse_to_basic() {
    let mock_server = MockServer::start().await;

    const SOAP_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <tds:GetDeviceInformationResponse xmlns:tds="http://www.onvif.org/ver10/device/wsdl">
      <tds:Manufacturer>Acme Corp</tds:Manufacturer>
      <tds:Model>Model-X</tds:Model>
      <tds:FirmwareVersion>2.0.1</tds:FirmwareVersion>
      <tds:SerialNumber>SN98765</tds:SerialNumber>
      <tds:HardwareId>HW-2</tds:HardwareId>
    </tds:GetDeviceInformationResponse>
  </s:Body>
</s:Envelope>"#;

    // First request has no Authorization header (WS-Security in body), mock server rejects with 401
    Mock::given(method("POST"))
        .and(|req: &wiremock::Request| !req.headers.contains_key("authorization"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&mock_server)
        .await;

    // Second request has Basic auth (admin:secret -> YWRtaW46c2VjcmV0), succeeds with 200
    Mock::given(method("POST"))
        .and(header("authorization", "Basic YWRtaW46c2VjcmV0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SOAP_RESPONSE))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let info = fetch_device_info(&client, &mock_server.uri(), "admin", "secret")
        .await
        .unwrap();

    assert_eq!(info.manufacturer, "Acme Corp");
    assert_eq!(info.model, "Model-X");
    assert_eq!(info.firmware, "2.0.1");
}

#[tokio::test]
async fn test_local_loopback_probe_responder() {
    // Bind a UDP socket to receive the probe and respond with a probe match
    let responder = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let responder_addr = responder.local_addr().unwrap();

    let responder_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        let (len, peer) = responder.recv_from(&mut buf).await.unwrap();
        assert!(len > 0);
        let request = String::from_utf8_lossy(&buf[..len]);
        assert!(request.contains("Probe"));

        let response = r#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <e:Body>
    <d:ProbeMatches>
      <d:ProbeMatch>
        <d:XAddrs>http://192.168.1.100:8000/onvif/device_service</d:XAddrs>
      </d:ProbeMatch>
    </d:ProbeMatches>
  </e:Body>
</e:Envelope>"#;

        responder.send_to(response.as_bytes(), peer).await.unwrap();
    });

    let devices = discover(
        Duration::from_millis(500),
        Some(&responder_addr.to_string()),
    )
    .await
    .unwrap();

    responder_task.await.unwrap();

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].host, "192.168.1.100:8000");
    assert_eq!(
        devices[0].address,
        "http://192.168.1.100:8000/onvif/device_service"
    );
}

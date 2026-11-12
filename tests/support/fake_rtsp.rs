#![allow(dead_code)]

use base64::Engine;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    None,
    Basic,
    Digest,
    Always401,
}

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub cseq: u32,
    pub auth: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    DropAfterPackets(usize),
    Stall,
}

#[derive(Debug, Clone)]
pub struct FakeRtspConfig {
    pub auth_mode: AuthMode,
    pub username: String,
    pub password: String,
    pub expected_path: String,
    pub fault: Fault,
    pub use_tls: bool,
    pub video_h264_frames: Vec<Vec<u8>>,
}

impl Default for FakeRtspConfig {
    fn default() -> Self {
        Self {
            auth_mode: AuthMode::None,
            username: "admin".into(),
            password: "secret".into(),
            expected_path: "/live".into(),
            fault: Fault::None,
            use_tls: false,
            video_h264_frames: Vec::new(),
        }
    }
}

pub struct FakeRtspServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    teardown_called: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    is_tls: bool,
}

impl FakeRtspServer {
    pub async fn start(config: FakeRtspConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let requests = Arc::new(Mutex::new(Vec::new()));
        let teardown_called = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let reqs_clone = Arc::clone(&requests);
        let teardown_clone = Arc::clone(&teardown_called);
        let shutdown_clone = Arc::clone(&shutdown);
        let is_tls = config.use_tls;

        tokio::spawn(async move {
            let tls_acceptor = if config.use_tls {
                Some(create_tls_acceptor())
            } else {
                None
            };

            while !shutdown_clone.load(Ordering::Relaxed) {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };

                let reqs = Arc::clone(&reqs_clone);
                let td = Arc::clone(&teardown_clone);
                let cfg = config.clone();

                if let Some(ref acceptor) = tls_acceptor {
                    let Ok(tls_stream) = acceptor.accept(stream).await else {
                        continue;
                    };
                    tokio::spawn(handle_connection(Box::new(tls_stream), cfg, reqs, td));
                } else {
                    tokio::spawn(handle_connection(Box::new(stream), cfg, reqs, td));
                }
            }
        });

        Self {
            addr,
            requests,
            teardown_called,
            shutdown,
            is_tls,
        }
    }

    pub fn url(&self) -> String {
        let scheme = if self.is_tls { "rtsps" } else { "rtsp" };
        format!("{scheme}://127.0.0.1:{}{}", self.addr.port(), "/live")
    }

    pub fn url_with_path(&self, path: &str) -> String {
        let scheme = if self.is_tls { "rtsps" } else { "rtsp" };
        let p = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("{scheme}://127.0.0.1:{}{p}", self.addr.port())
    }

    pub fn received_requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }

    pub fn teardown_received(&self) -> bool {
        self.teardown_called.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for FakeRtspServer {
    fn drop(&mut self) {
        self.stop();
    }
}

trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> AsyncStream for T {}

async fn handle_connection(
    mut stream: Box<dyn AsyncStream>,
    config: FakeRtspConfig,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    teardown_called: Arc<AtomicBool>,
) {
    let mut buf = vec![0u8; 4096];
    let mut interleaved_video_channel: Option<u8> = None;
    let mut udp_peer: Option<SocketAddr> = None;
    let seq_counter = AtomicU32::new(1);

    loop {
        if config.fault == Fault::Stall {
            // Hang indefinitely
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            return;
        }

        let n = match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        let req_str = String::from_utf8_lossy(&buf[..n]);
        let mut lines = req_str.lines();
        let Some(req_line) = lines.next() else {
            continue;
        };

        let parts: Vec<&str> = req_line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let method = parts[0];
        let req_uri = parts[1];
        let req_path = if let Ok(u) = url::Url::parse(req_uri) {
            u.path().to_string()
        } else {
            req_uri.to_string()
        };

        let mut cseq = 0;
        let mut auth_hdr = None;
        let mut transport_hdr = None;

        for line in lines {
            if let Some(val) = line.strip_prefix("CSeq:") {
                cseq = val.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(val) = line.strip_prefix("Authorization:") {
                auth_hdr = Some(val.trim().to_string());
            } else if let Some(val) = line.strip_prefix("Transport:") {
                transport_hdr = Some(val.trim().to_string());
            }
        }

        requests.lock().unwrap().push(RecordedRequest {
            method: method.to_string(),
            path: req_path.clone(),
            cseq,
            auth: auth_hdr.clone(),
        });

        match method {
            "OPTIONS" => {
                let resp = format!(
                    "RTSP/1.0 200 OK\r\n\
                     CSeq: {cseq}\r\n\
                     Public: OPTIONS, DESCRIBE, SETUP, TEARDOWN, PLAY, PAUSE\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
            "DESCRIBE" => {
                // Auth check
                let authed = match config.auth_mode {
                    AuthMode::None => true,
                    AuthMode::Always401 => false,
                    AuthMode::Basic => {
                        let want = format!(
                            "Basic {}",
                            base64::prelude::BASE64_STANDARD
                                .encode(format!("{}:{}", config.username, config.password))
                        );
                        auth_hdr.as_deref() == Some(&want)
                    }
                    AuthMode::Digest => {
                        // Accept any digest authorization header containing username
                        if let Some(ref a) = auth_hdr {
                            a.starts_with("Digest") && a.contains(&config.username)
                        } else {
                            false
                        }
                    }
                };

                if !authed {
                    let challenge = match config.auth_mode {
                        AuthMode::Basic => "WWW-Authenticate: Basic realm=\"qam\"\r\n",
                        _ => {
                            "WWW-Authenticate: Digest realm=\"qam\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", qop=\"auth\"\r\n"
                        }
                    };
                    let resp = format!(
                        "RTSP/1.0 401 Unauthorized\r\n\
                         CSeq: {cseq}\r\n\
                         {challenge}\r\n"
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    continue;
                }

                // Path check
                if !config.expected_path.is_empty() && req_path != config.expected_path {
                    let resp = format!(
                        "RTSP/1.0 404 Not Found\r\n\
                         CSeq: {cseq}\r\n\r\n"
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    continue;
                }

                // Return 200 with SDP
                let sdp = "v=0\r\n\
                     o=- 12345678 1 IN IP4 127.0.0.1\r\n\
                     s=qam-test\r\n\
                     t=0 0\r\n\
                     m=video 0 RTP/AVP 96\r\n\
                     c=IN IP4 0.0.0.0\r\n\
                     a=rtpmap:96 H264/90000\r\n\
                     a=control:track0\r\n\
                     m=audio 0 RTP/AVP 8\r\n\
                     c=IN IP4 0.0.0.0\r\n\
                     a=rtpmap:8 PCMA/8000\r\n\
                     a=control:track1\r\n";
                let resp = format!(
                    "RTSP/1.0 200 OK\r\n\
                     CSeq: {cseq}\r\n\
                     Content-Type: application/sdp\r\n\
                     Content-Length: {}\r\n\r\n\
                     {sdp}",
                    sdp.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
            "SETUP" => {
                let trans = transport_hdr.unwrap_or_default();
                let resp_transport = if trans.contains("interleaved") {
                    interleaved_video_channel = Some(0);
                    "RTP/AVP/TCP;unicast;interleaved=0-1"
                } else if let Some(idx) = trans.find("client_port=") {
                    // UDP transport: parse client ports
                    let port_part = &trans[idx + 12..];
                    let ports: Vec<&str> = port_part.split('-').collect();
                    let client_rtp_port = ports[0]
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        .unwrap_or("0")
                        .parse::<u16>()
                        .unwrap_or(0);

                    udp_peer = Some(SocketAddr::new(
                        "127.0.0.1".parse().unwrap(),
                        client_rtp_port,
                    ));
                    "RTP/AVP/UDP;unicast;client_port=5000-5001;server_port=6000-6001"
                } else {
                    interleaved_video_channel = Some(0);
                    "RTP/AVP/TCP;unicast;interleaved=0-1"
                };

                let resp = format!(
                    "RTSP/1.0 200 OK\r\n\
                     CSeq: {cseq}\r\n\
                     Session: 12345678;timeout=60\r\n\
                     Transport: {resp_transport}\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
            "PLAY" => {
                let resp = format!(
                    "RTSP/1.0 200 OK\r\n\
                     CSeq: {cseq}\r\n\
                     Session: 12345678\r\n\
                     RTP-Info: url=track0;seq=1;rtptime=1000\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;

                // Stream video frames if present
                if let Some(ch) = interleaved_video_channel {
                    let mut packets_sent = 0;
                    for (frame_idx, frame) in config.video_h264_frames.iter().enumerate() {
                        let rtp_pkts =
                            create_h264_rtp_packets(frame, (frame_idx * 9000) as u32, &seq_counter);
                        for pkt in rtp_pkts {
                            if let Fault::DropAfterPackets(max_pkts) = config.fault
                                && packets_sent >= max_pkts
                            {
                                return;
                            }

                            // Write '$' + channel + length (2 bytes be) + rtp
                            let mut packet_bytes = Vec::with_capacity(4 + pkt.len());
                            packet_bytes.push(b'$');
                            packet_bytes.push(ch);
                            packet_bytes.extend_from_slice(&(pkt.len() as u16).to_be_bytes());
                            packet_bytes.extend_from_slice(&pkt);

                            if stream.write_all(&packet_bytes).await.is_err() {
                                return;
                            }
                            packets_sent += 1;
                        }
                    }
                } else if let Some(peer) = udp_peer {
                    // Send over UDP socket
                    if let Ok(udp) = UdpSocket::bind("127.0.0.1:0").await {
                        for (frame_idx, frame) in config.video_h264_frames.iter().enumerate() {
                            let rtp_pkts = create_h264_rtp_packets(
                                frame,
                                (frame_idx * 9000) as u32,
                                &seq_counter,
                            );
                            for pkt in rtp_pkts {
                                let _ = udp.send_to(&pkt, peer).await;
                            }
                        }
                    }
                }
            }
            "TEARDOWN" => {
                teardown_called.store(true, Ordering::SeqCst);
                let resp = format!(
                    "RTSP/1.0 200 OK\r\n\
                     CSeq: {cseq}\r\n\
                     Session: 12345678\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                return;
            }
            _ => {
                let resp = format!(
                    "RTSP/1.0 501 Not Implemented\r\n\
                     CSeq: {cseq}\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        }
    }
}

/// Creates RFC 6184 RTP packets for an Annex-B H.264 frame.
fn create_h264_rtp_packets(frame: &[u8], timestamp: u32, seq: &AtomicU32) -> Vec<Vec<u8>> {
    let nals = qam::media::annexb::find_nal_units(frame);
    let mut packets = Vec::new();

    for (nal_idx, nal) in nals.iter().enumerate() {
        if nal.is_empty() {
            continue;
        }

        let is_last_nal = nal_idx == nals.len() - 1;

        if nal.len() <= 1400 {
            // Single NAL unit packet
            let seq_num = (seq.fetch_add(1, Ordering::Relaxed) & 0xFFFF) as u16;
            let marker = if is_last_nal { 1u8 } else { 0u8 };

            let mut pkt = Vec::with_capacity(12 + nal.len());
            // RTP Header
            pkt.push(0x80); // V=2, P=0, X=0, CC=0
            pkt.push((marker << 7) | 96); // M, PT=96
            pkt.extend_from_slice(&seq_num.to_be_bytes());
            pkt.extend_from_slice(&timestamp.to_be_bytes());
            pkt.extend_from_slice(&12345678u32.to_be_bytes()); // SSRC
            pkt.extend_from_slice(nal);
            packets.push(pkt);
        } else {
            // Fragmentation Unit (FU-A)
            let nal_header = nal[0];
            let nal_type = nal_header & 0x1F;
            let nri = nal_header & 0x60;
            let fu_indicator = nri | 28; // FU-A indicator type = 28

            let payload = &nal[1..];
            let chunks: Vec<&[u8]> = payload.chunks(1380).collect();

            for (chunk_idx, chunk) in chunks.iter().enumerate() {
                let seq_num = (seq.fetch_add(1, Ordering::Relaxed) & 0xFFFF) as u16;
                let is_start = chunk_idx == 0;
                let is_end = chunk_idx == chunks.len() - 1;
                let marker = if is_last_nal && is_end { 1u8 } else { 0u8 };

                let start_bit = if is_start { 0x80 } else { 0x00 };
                let end_bit = if is_end { 0x40 } else { 0x00 };
                let fu_header = start_bit | end_bit | nal_type;

                let mut pkt = Vec::with_capacity(12 + 2 + chunk.len());
                pkt.push(0x80);
                pkt.push((marker << 7) | 96);
                pkt.extend_from_slice(&seq_num.to_be_bytes());
                pkt.extend_from_slice(&timestamp.to_be_bytes());
                pkt.extend_from_slice(&12345678u32.to_be_bytes());
                pkt.push(fu_indicator);
                pkt.push(fu_header);
                pkt.extend_from_slice(chunk);
                packets.push(pkt);
            }
        }
    }

    packets
}

fn create_tls_acceptor() -> tokio_rustls::TlsAcceptor {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let cert =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let cert_der = CertificateDer::from(cert.cert);
    let key_der = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .unwrap();

    tokio_rustls::TlsAcceptor::from(Arc::new(server_config))
}

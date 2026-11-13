use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

use retina::client::{
    AuthPolicy, Credentials, Demuxed, PlayOptions, Session, SessionGroup, SessionOptions,
    SetupOptions, TcpTransportOptions, Transport as RetinaTransport, UdpTransportOptions,
};
use retina::codec::CodecItem;
pub use retina::codec::FrameFormat;

use crate::cli::flags::{RtspAuth, Transport};
use crate::errors::QamError;
use crate::media::codec::{AudioCodec, VideoCodec};

#[derive(Debug, Clone)]
pub enum MediaPayload {
    Video {
        codec: VideoCodec,
        is_keyframe: bool,
        data: Vec<u8>,
        has_new_parameters: bool,
    },
    Audio {
        codec: AudioCodec,
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct MediaFrame {
    pub stream_id: usize,
    pub timestamp_ticks: i64,
    pub clock_rate: u32,
    pub payload: MediaPayload,
}

impl MediaFrame {
    pub fn is_video(&self) -> bool {
        matches!(self.payload, MediaPayload::Video { .. })
    }

    pub fn is_audio(&self) -> bool {
        matches!(self.payload, MediaPayload::Audio { .. })
    }
}

pub struct RtspSession {
    demuxed: Demuxed,
    session_group: Arc<SessionGroup>,
    pub video_stream_id: Option<usize>,
    pub audio_stream_id: Option<usize>,
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
    pub video_clock_rate: u32,
    pub audio_clock_rate: u32,
    io_timeout: Duration,
}

impl std::fmt::Debug for RtspSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtspSession")
            .field("video_stream_id", &self.video_stream_id)
            .field("audio_stream_id", &self.audio_stream_id)
            .field("video_codec", &self.video_codec)
            .field("audio_codec", &self.audio_codec)
            .field("video_clock_rate", &self.video_clock_rate)
            .field("audio_clock_rate", &self.audio_clock_rate)
            .field("io_timeout", &self.io_timeout)
            .finish()
    }
}

impl RtspSession {
    pub async fn connect(
        url_str: &str,
        transport: Transport,
        auth: RtspAuth,
        io_timeout: Duration,
        want_audio: bool,
        format: FrameFormat,
    ) -> Result<Self, QamError> {
        let mut parsed_url =
            Url::parse(url_str).map_err(|e| QamError::RtspUrl(format!("invalid RTSP URL: {e}")))?;

        // Extract credentials and remove them from the URL so retina doesn't reject them
        let user = if !parsed_url.username().is_empty() {
            let u = urlencoding_decode(parsed_url.username());
            let _ = parsed_url.set_username("");
            Some(u)
        } else {
            None
        };

        let pass = if let Some(p) = parsed_url.password() {
            let p_dec = urlencoding_decode(p);
            let _ = parsed_url.set_password(None);
            p_dec
        } else {
            String::new()
        };

        let creds = user.map(|u| Credentials {
            username: u,
            password: pass,
        });

        let auth_policy = match auth {
            RtspAuth::Auto => AuthPolicy::Auto,
            RtspAuth::Basic => AuthPolicy::Basic,
            RtspAuth::Digest => AuthPolicy::Digest,
        };

        let session_group = Arc::new(SessionGroup::default());
        let options = SessionOptions::default()
            .creds(creds)
            .auth_policy(auth_policy)
            .user_agent(format!("qam/{}", crate::version::VERSION))
            .session_group(Arc::clone(&session_group))
            .session_id(retina::client::SessionIdPolicy::UseFirst)
            .teardown(retina::client::TeardownPolicy::Always);

        // Connect and DESCRIBE
        let describe_fut = Session::describe(parsed_url, options);
        let mut session = tokio::time::timeout(io_timeout, describe_fut)
            .await
            .map_err(|_| {
                QamError::Media(format!(
                    "timed out connecting/describing RTSP session at {} (transport={}, timeout={}s)\nHints:\n  - Verify the camera is reachable on the network and the host/port are correct.\n  - Try increasing --timeout or switching transport (--rtsp-transport udp).\n  - Run `qam doctor --probe` to diagnose connectivity.",
                    crate::rtsp_url::redact_url(url_str),
                    transport.as_str(),
                    io_timeout.as_secs()
                ))
            })?
            .map_err(|e| {
                QamError::Media(format_rtsp_describe_error(url_str, transport, auth, &e))
            })?;

        let mut video_stream_id = None;
        let mut video_codec = None;
        let mut video_clock_rate = 90000;

        let mut audio_stream_id = None;
        let mut audio_codec = None;
        let mut audio_clock_rate = 8000;

        for (idx, stream) in session.streams().iter().enumerate() {
            let media = stream.media();
            let enc = stream.encoding_name().to_ascii_lowercase();
            let clock_rate = stream.clock_rate_hz();

            if media == "video" && video_stream_id.is_none() {
                if enc == "h264" || enc == "avc" {
                    video_stream_id = Some(idx);
                    video_codec = Some(VideoCodec::H264);
                    video_clock_rate = clock_rate;
                } else if enc == "h265" || enc == "hevc" {
                    video_stream_id = Some(idx);
                    video_codec = Some(VideoCodec::H265);
                    video_clock_rate = clock_rate;
                } else if enc == "jpeg" || enc == "mjpeg" {
                    video_stream_id = Some(idx);
                    video_codec = Some(VideoCodec::Mjpeg);
                    video_clock_rate = clock_rate;
                }
            } else if media == "audio" && want_audio && audio_stream_id.is_none() {
                if enc == "pcma" {
                    audio_stream_id = Some(idx);
                    audio_codec = Some(AudioCodec::Pcma);
                    audio_clock_rate = clock_rate;
                } else if enc == "pcmu" {
                    audio_stream_id = Some(idx);
                    audio_codec = Some(AudioCodec::Pcmu);
                    audio_clock_rate = clock_rate;
                } else if enc == "l16" {
                    audio_stream_id = Some(idx);
                    audio_codec = Some(AudioCodec::L16);
                    audio_clock_rate = clock_rate;
                } else if enc.starts_with("g726") {
                    audio_stream_id = Some(idx);
                    audio_codec = Some(AudioCodec::G726);
                    audio_clock_rate = clock_rate;
                } else if enc == "opus" {
                    audio_stream_id = Some(idx);
                    audio_codec = Some(AudioCodec::Opus);
                    audio_clock_rate = clock_rate;
                } else if enc == "mpeg4-generic" || enc == "aac" || enc == "mp4a-latm" {
                    audio_stream_id = Some(idx);
                    audio_codec = Some(AudioCodec::Aac);
                    audio_clock_rate = clock_rate;
                }
            }
        }

        // SETUP streams
        let retina_transport = match transport {
            Transport::Tcp => RetinaTransport::Tcp(TcpTransportOptions::default()),
            Transport::Udp => RetinaTransport::Udp(UdpTransportOptions::default()),
        };

        if let Some(vid_idx) = video_stream_id {
            let setup_opts = SetupOptions::default()
                .transport(retina_transport.clone())
                .frame_format(format);
            let setup_fut = session.setup(vid_idx, setup_opts);
            tokio::time::timeout(io_timeout, setup_fut)
                .await
                .map_err(|_| QamError::Media("timed out during RTSP video setup".to_string()))?
                .map_err(|e| QamError::Media(format!("RTSP video setup failed: {e}")))?;
        }

        if let Some(aud_idx) = audio_stream_id {
            let setup_opts = SetupOptions::default()
                .transport(retina_transport)
                .frame_format(format);
            let setup_fut = session.setup(aud_idx, setup_opts);
            tokio::time::timeout(io_timeout, setup_fut)
                .await
                .map_err(|_| QamError::Media("timed out during RTSP audio setup".to_string()))?
                .map_err(|e| QamError::Media(format!("RTSP audio setup failed: {e}")))?;
        }

        // PLAY
        let play_options = PlayOptions::default()
            .initial_timestamp(retina::client::InitialTimestampPolicy::Permissive);
        let play_fut = session.play(play_options);
        let playing_session = tokio::time::timeout(io_timeout, play_fut)
            .await
            .map_err(|_| QamError::Media("timed out during RTSP play".to_string()))?
            .map_err(|e| QamError::Media(format!("RTSP play failed: {e}")))?;

        let demuxed = playing_session
            .demuxed()
            .map_err(|e| QamError::Media(format!("failed to initialize demuxer: {e}")))?;

        Ok(Self {
            demuxed,
            session_group,
            video_stream_id,
            audio_stream_id,
            video_codec,
            audio_codec,
            video_clock_rate,
            audio_clock_rate,
            io_timeout,
        })
    }

    /// Reads the next media frame from the RTSP session.
    pub async fn next_frame(&mut self) -> Result<Option<MediaFrame>, QamError> {
        loop {
            let next_fut = self.demuxed.next();
            let item = match tokio::time::timeout(self.io_timeout, next_fut).await {
                Ok(Some(Ok(item))) => item,
                Ok(Some(Err(e))) => return Err(QamError::Media(format!("RTSP stream error: {e}"))),
                Ok(None) => return Ok(None),
                Err(_) => {
                    return Err(QamError::Media("timed out waiting for frame".to_string()));
                }
            };

            match item {
                CodecItem::VideoFrame(vf) => {
                    if let Some(vid_idx) = self.video_stream_id
                        && vf.stream_id() == vid_idx
                        && let Some(codec) = self.video_codec
                    {
                        let has_new_parameters = vf.has_new_parameters();
                        let is_keyframe = vf.is_random_access_point();
                        let ts = vf.timestamp().timestamp();
                        let data = vf.into_data();
                        return Ok(Some(MediaFrame {
                            stream_id: vid_idx,
                            timestamp_ticks: ts,
                            clock_rate: self.video_clock_rate,
                            payload: MediaPayload::Video {
                                codec,
                                is_keyframe,
                                data,
                                has_new_parameters,
                            },
                        }));
                    }
                }
                CodecItem::AudioFrame(af) => {
                    if let Some(aud_idx) = self.audio_stream_id
                        && af.stream_id() == aud_idx
                        && let Some(codec) = self.audio_codec
                    {
                        return Ok(Some(MediaFrame {
                            stream_id: aud_idx,
                            timestamp_ticks: af.timestamp().timestamp(),
                            clock_rate: self.audio_clock_rate,
                            payload: MediaPayload::Audio {
                                codec,
                                data: af.data().to_vec(),
                            },
                        }));
                    }
                }
                _ => {}
            }
        }
    }

    /// Explicitly tears down the session and waits for confirmation.
    pub async fn teardown(self) {
        drop(self.demuxed);
        let _ =
            tokio::time::timeout(Duration::from_secs(2), self.session_group.await_teardown()).await;
    }
}

fn format_rtsp_describe_error(
    url_str: &str,
    transport: Transport,
    auth: RtspAuth,
    err: &retina::Error,
) -> String {
    let redacted = crate::rtsp_url::redact_url(url_str);
    let raw = err.to_string();
    let summary = raw.lines().next().unwrap_or(&raw).trim();
    let status = err.status_code();

    let hints = match status {
        Some(454) => {
            "  - Camera returned 454 (Session Not Found) to DESCRIBE.\n\
             \x20 - Verify the stream path (--stream or --path): many cameras use /live, /h264Preview_01_main, /ch0_0.h264, or /11 instead of /stream1.\n\
             \x20 - If this camera works behind go2rtc, copy the exact path from your working RTSP URL into --path (or 'path:' in config.yaml).\n\
             \x20 - Try --rtsp-transport udp or --rtsp-auth basic|digest, or run `qam doctor --probe`."
        }
        Some(401) => {
            "  - Authentication failed (401 Unauthorized). Check username and password in config or `qam add`.\n\
             \x20 - If the password contains special characters (e.g. , $ ! & @ :), wrap it in single quotes in your shell or quotes in config.yaml.\n\
             \x20 - Try forcing --rtsp-auth basic or --rtsp-auth digest."
        }
        Some(404) => {
            "  - Stream path not found (404). Check --stream (e.g. stream1, stream2) or specify the exact path with --path."
        }
        _ => {
            "  - Verify host, port, and stream path (--stream / --path) in your camera config.\n\
             \x20 - If the password contains special characters (e.g. , $ ! & @ :), ensure it is quoted in config.yaml.\n\
             \x20 - Try --rtsp-transport udp or --rtsp-auth basic|digest, or run `qam doctor --probe`."
        }
    };

    format!(
        "RTSP describe failed for {redacted}: {summary}\nHints:\n  - Active settings: transport={}, auth={}\n{hints}",
        transport.as_str(),
        auth.as_str()
    )
}

fn urlencoding_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

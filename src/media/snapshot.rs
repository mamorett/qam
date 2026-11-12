use std::path::Path;
use std::time::Duration;

use retina::codec::FrameFormat;

use crate::cli::flags::{RtspAuth, Transport};
use crate::errors::QamError;
use crate::media::image_out::{detect_format_from_path, encode_image, write_image_file};
use crate::media::session::{MediaPayload, RtspSession};
use crate::media::video_decode::VideoDecoder;

pub struct SnapshotOptions<'a> {
    pub url: &'a str,
    pub transport: Transport,
    pub auth: RtspAuth,
    pub timeout: Duration,
    pub out_path: &'a str,
    pub overwrite: bool,
    pub quality: u8,
}

/// Captures a single video snapshot from an RTSP stream and saves it to disk or stdout.
pub async fn capture_snapshot(opts: SnapshotOptions<'_>) -> Result<(), QamError> {
    let mut session = RtspSession::connect(
        opts.url,
        opts.transport,
        opts.auth,
        opts.timeout,
        false, // want_audio = false
        FrameFormat::SIMPLE,
    )
    .await?;

    let video_codec = session
        .video_codec
        .ok_or_else(|| QamError::Media("no video stream found in RTSP presentation".into()))?;

    let mut decoder = VideoDecoder::new(video_codec)?;

    let mut decoded_img = None;
    let deadline = tokio::time::Instant::now() + opts.timeout;

    while tokio::time::Instant::now() < deadline {
        let frame_fut = session.next_frame();
        let frame_opt = tokio::select! {
            _ = crate::signals::wait_for_shutdown() => {
                session.teardown().await;
                if opts.out_path != "-" {
                    let _ = std::fs::remove_file(opts.out_path);
                }
                return Err(QamError::Interrupted(130));
            }
            res = frame_fut => res?,
        };
        let Some(frame) = frame_opt else {
            break;
        };

        if let MediaPayload::Video {
            data, is_keyframe, ..
        } = frame.payload
        {
            // Prefer keyframes for instant clean decode, but try decode if keyframe
            if is_keyframe && let Some(img) = decoder.decode_frame(&data)? {
                decoded_img = Some(img);
                break;
            }
        }
    }

    session.teardown().await;

    let img = decoded_img
        .ok_or_else(|| QamError::Media("no video keyframe received before timeout".into()))?;

    let format = detect_format_from_path(Path::new(opts.out_path));
    let encoded = encode_image(&img, format, opts.quality)?;
    write_image_file(&encoded, opts.out_path, opts.overwrite)?;

    Ok(())
}

use std::path::Path;
use std::time::Duration;

use retina::codec::FrameFormat;

use crate::cli::flags::{RtspAuth, Transport};
use crate::errors::QamError;
use crate::media::annexb::{annex_b_to_avcc, extract_h264_sps_pps};
use crate::media::audio::decode_g711_to_s16le;
use crate::media::codec::AudioCodec;
use crate::media::mux::mp4::Muxer;
use crate::media::mux::wav::write_wav_s16le;
use crate::media::session::{MediaPayload, RtspSession};
use crate::media::video_decode::VideoDecoder;

pub struct ClipRecordOptions<'a> {
    pub url: &'a str,
    pub transport: Transport,
    pub auth: RtspAuth,
    pub timeout: Duration,
    pub duration: Duration,
    pub out_path: &'a str,
    pub overwrite: bool,
    pub no_audio: bool,
    pub audio_only: bool,
}

/// Records a video clip (or audio-only clip) from an RTSP stream and writes to disk.
pub async fn record_media(opts: ClipRecordOptions<'_>) -> Result<(), QamError> {
    if opts.out_path == "-" {
        let label = if opts.audio_only { "audio" } else { "clip" };
        return Err(QamError::Usage(format!(
            "stdout output not supported for {label}"
        )));
    }

    let dest = Path::new(opts.out_path);
    if dest.exists() && !opts.overwrite {
        return Err(QamError::Media(format!(
            "output file {:?} already exists (use --overwrite to replace)",
            opts.out_path
        )));
    }

    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = format!("{}.tmp.{}", opts.out_path, std::process::id());
    let tmp = Path::new(&tmp_path);

    if opts.audio_only {
        record_audio_stream(&opts, tmp).await?;
    } else {
        record_video_clip(&opts, tmp).await?;
    }

    std::fs::rename(tmp, dest)?;
    Ok(())
}

async fn record_video_clip(opts: &ClipRecordOptions<'_>, tmp_path: &Path) -> Result<(), QamError> {
    let mut session = RtspSession::connect(
        opts.url,
        opts.transport,
        opts.auth,
        opts.timeout,
        !opts.no_audio,
        FrameFormat::SIMPLE,
    )
    .await?;

    let video_codec = session
        .video_codec
        .ok_or_else(|| QamError::Media("no video stream found in RTSP presentation".into()))?;

    let mut decoder = VideoDecoder::new(video_codec)?;
    let mut muxer = None;
    let mut last_ts = 0u64;

    let start_time = tokio::time::Instant::now();
    let deadline = start_time + opts.duration;

    while tokio::time::Instant::now() < deadline {
        let frame_opt = match session.next_frame().await {
            Ok(f) => f,
            Err(e) => {
                if muxer.is_some() {
                    break;
                } else {
                    return Err(e);
                }
            }
        };
        let Some(frame) = frame_opt else {
            break;
        };

        match frame.payload {
            MediaPayload::Video {
                data, is_keyframe, ..
            } => {
                if muxer.is_none() {
                    if !is_keyframe {
                        continue;
                    }

                    // Decode first keyframe to get dimensions
                    let (width, height) = if let Some(img) = decoder.decode_frame(&data)? {
                        (img.width() as u16, img.height() as u16)
                    } else {
                        (640, 480)
                    };

                    let (sps, pps) = extract_h264_sps_pps(&data);
                    let sps_bytes = sps.unwrap_or_else(|| vec![0x67, 0x42, 0x00, 0x1E]);
                    let pps_bytes = pps.unwrap_or_else(|| vec![0x68, 0xCE, 0x3C, 0x80]);

                    let mut m = Muxer::create(tmp_path, 90000)?;
                    m.add_video_track(video_codec, width, height, 90000, &sps_bytes, &pps_bytes)?;
                    muxer = Some(m);
                }

                if let Some(ref mut m) = muxer {
                    let avcc = annex_b_to_avcc(&data);
                    let sample_dur = 3000; // ~30 fps
                    m.write_video_sample(avcc, last_ts, sample_dur, is_keyframe)?;
                    last_ts += sample_dur as u64;
                }
            }
            MediaPayload::Audio { .. } => {
                // Audio muxing if enabled
            }
        }
    }

    session.teardown().await;

    if let Some(ref mut m) = muxer {
        m.finalize()?;
    } else {
        return Err(QamError::Media(
            "no video keyframe received before duration elapsed".into(),
        ));
    }

    Ok(())
}

async fn record_audio_stream(
    opts: &ClipRecordOptions<'_>,
    tmp_path: &Path,
) -> Result<(), QamError> {
    let mut session = RtspSession::connect(
        opts.url,
        opts.transport,
        opts.auth,
        opts.timeout,
        true, // want_audio = true
        FrameFormat::SIMPLE,
    )
    .await?;

    let _audio_codec = session
        .audio_codec
        .ok_or_else(|| QamError::Media("no audio stream found in RTSP presentation".into()))?;

    let clock_rate = if session.audio_clock_rate > 0 {
        session.audio_clock_rate
    } else {
        8000
    };

    let mut pcm_samples = Vec::new();
    let start_time = tokio::time::Instant::now();
    let deadline = start_time + opts.duration;

    while tokio::time::Instant::now() < deadline {
        let frame_opt = match session.next_frame().await {
            Ok(f) => f,
            Err(e) => {
                if !pcm_samples.is_empty() {
                    break;
                } else {
                    return Err(e);
                }
            }
        };
        let Some(frame) = frame_opt else {
            break;
        };

        if let MediaPayload::Audio { data, codec } = frame.payload {
            match codec {
                AudioCodec::Pcma => {
                    pcm_samples.extend(decode_g711_to_s16le(&data, true));
                }
                AudioCodec::Pcmu => {
                    pcm_samples.extend(decode_g711_to_s16le(&data, false));
                }
                _ => {
                    // For PCM / other, treat as 16-bit
                    for &chunk in data.as_chunks::<2>().0 {
                        pcm_samples.push(i16::from_le_bytes(chunk));
                    }
                }
            }
        }
    }

    session.teardown().await;

    if pcm_samples.is_empty() {
        // Generate at least one second of silence if no audio frames were received
        pcm_samples = vec![0i16; 8000];
    }

    write_wav_s16le(tmp_path, &pcm_samples, clock_rate, 1)?;
    Ok(())
}

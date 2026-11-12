use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

use mp4::{
    AvcConfig, FourCC, HevcConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig,
    TrackType,
};

use crate::errors::QamError;
use crate::media::codec::VideoCodec;

pub struct Muxer<W: Write + Seek> {
    writer: Mp4Writer<W>,
    video_track_id: Option<u32>,
    #[allow(dead_code)]
    audio_track_id: Option<u32>,
    video_timescale: u32,
    #[allow(dead_code)]
    audio_timescale: u32,
}

impl Muxer<BufWriter<File>> {
    pub fn create(path: &Path, timescale: u32) -> Result<Self, QamError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(path)?;
        let buf = BufWriter::new(file);

        let config = Mp4Config {
            major_brand: FourCC::from(*b"isom"),
            minor_version: 512,
            compatible_brands: vec![
                FourCC::from(*b"isom"),
                FourCC::from(*b"iso2"),
                FourCC::from(*b"avc1"),
                FourCC::from(*b"mp41"),
            ],
            timescale,
        };

        let writer = Mp4Writer::write_start(buf, &config)
            .map_err(|e| QamError::Media(format!("failed to start MP4 muxer: {e}")))?;

        Ok(Self {
            writer,
            video_track_id: None,
            audio_track_id: None,
            video_timescale: timescale,
            audio_timescale: 8000,
        })
    }
}

impl<W: Write + Seek> Muxer<W> {
    pub fn add_video_track(
        &mut self,
        codec: VideoCodec,
        width: u16,
        height: u16,
        timescale: u32,
        sps: &[u8],
        pps: &[u8],
    ) -> Result<u32, QamError> {
        let media_conf = match codec {
            VideoCodec::H264 => MediaConfig::AvcConfig(AvcConfig {
                width,
                height,
                seq_param_set: sps.to_vec(),
                pic_param_set: pps.to_vec(),
            }),
            VideoCodec::H265 => MediaConfig::HevcConfig(HevcConfig { width, height }),
            _ => {
                return Err(QamError::Media(format!(
                    "codec {codec} not supported in MP4 container"
                )));
            }
        };

        let track_conf = TrackConfig {
            track_type: TrackType::Video,
            timescale,
            language: "und".to_string(),
            media_conf,
        };

        self.writer
            .add_track(&track_conf)
            .map_err(|e| QamError::Media(format!("failed to add video track to MP4: {e}")))?;

        let track_id = 1;
        self.video_track_id = Some(track_id);
        self.video_timescale = timescale;
        Ok(track_id)
    }

    pub fn write_video_sample(
        &mut self,
        data: Vec<u8>,
        start_time: u64,
        duration: u32,
        is_keyframe: bool,
    ) -> Result<(), QamError> {
        let track_id = self
            .video_track_id
            .ok_or_else(|| QamError::Media("video track not configured in MP4".into()))?;

        let sample = Mp4Sample {
            start_time,
            duration,
            rendering_offset: 0,
            is_sync: is_keyframe,
            bytes: bytes::Bytes::from(data),
        };

        self.writer
            .write_sample(track_id, &sample)
            .map_err(|e| QamError::Media(format!("failed to write video sample: {e}")))?;

        Ok(())
    }

    pub fn finalize(&mut self) -> Result<(), QamError> {
        self.writer
            .write_end()
            .map_err(|e| QamError::Media(format!("failed to finalize MP4 file: {e}")))?;
        Ok(())
    }
}

use image::RgbImage;
use openh264::decoder::Decoder as H264Decoder;
use openh264::formats::YUVSource;

use crate::errors::QamError;
use crate::media::codec::VideoCodec;
use crate::media::yuv::yuv420p_to_rgb;

pub struct VideoDecoder {
    codec: VideoCodec,
    h264_decoder: Option<H264Decoder>,
    h265_decoder: Option<rust_h265::Decoder>,
}

#[derive(Debug, Clone)]
pub struct DecodedYuv {
    pub width: usize,
    pub height: usize,
    pub y_plane: Vec<u8>,
}

impl VideoDecoder {
    pub fn new(codec: VideoCodec) -> Result<Self, QamError> {
        let (h264_decoder, h265_decoder) =
            match codec {
                VideoCodec::H264 => (
                    Some(H264Decoder::new().map_err(|e| {
                        QamError::Media(format!("failed to init H264 decoder: {e}"))
                    })?),
                    None,
                ),
                VideoCodec::H265 => (None, Some(rust_h265::Decoder::new().with_display_order())),
                VideoCodec::Mjpeg => (None, None),
            };

        Ok(Self {
            codec,
            h264_decoder,
            h265_decoder,
        })
    }

    /// Decodes a single video packet into an RGB image.
    /// Returns `Ok(Some(RgbImage))` if a picture was decoded, or `Ok(None)` if more data is needed.
    pub fn decode_frame(&mut self, data: &[u8]) -> Result<Option<RgbImage>, QamError> {
        match self.codec {
            VideoCodec::H264 => {
                let dec = self
                    .h264_decoder
                    .as_mut()
                    .ok_or_else(|| QamError::Media("H264 decoder not initialized".into()))?;

                match dec.decode(data) {
                    Ok(Some(yuv)) => {
                        let (width, height) = yuv.dimensions();
                        let (y_stride, u_stride, v_stride) = yuv.strides();
                        let img = yuv420p_to_rgb(
                            width,
                            height,
                            yuv.y(),
                            y_stride,
                            yuv.u(),
                            u_stride,
                            yuv.v(),
                            v_stride,
                        );
                        Ok(Some(img))
                    }
                    Ok(None) => Ok(None),
                    Err(e) => Err(QamError::Media(format!("H264 decode error: {e}"))),
                }
            }
            VideoCodec::H265 => {
                let dec = self
                    .h265_decoder
                    .as_mut()
                    .ok_or_else(|| QamError::Media("H265 decoder not initialized".into()))?;

                for nal in rust_h265::parse_annex_b(data) {
                    dec.decode_nal(&nal)
                        .map_err(|e| QamError::Media(format!("H265 NAL decode error: {e}")))?;

                    if let Some(frame) = dec.next_output() {
                        return Ok(Some(convert_h265_frame(&frame)?));
                    }
                }

                if let Some(frame) = dec.flush() {
                    return Ok(Some(convert_h265_frame(&frame)?));
                }

                Ok(None)
            }
            VideoCodec::Mjpeg => {
                let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
                    .map_err(|e| QamError::Media(format!("MJPEG decode error: {e}")))?
                    .to_rgb8();
                Ok(Some(img))
            }
        }
    }

    /// Decodes a single video packet and extracts only the luma (Y) plane.
    /// This is significantly faster for scene/motion detection since no RGB conversion is needed.
    pub fn decode_y_plane(&mut self, data: &[u8]) -> Result<Option<DecodedYuv>, QamError> {
        match self.codec {
            VideoCodec::H264 => {
                let dec = self
                    .h264_decoder
                    .as_mut()
                    .ok_or_else(|| QamError::Media("H264 decoder not initialized".into()))?;

                match dec.decode(data) {
                    Ok(Some(yuv)) => {
                        let (width, height) = yuv.dimensions();
                        let (y_stride, _, _) = yuv.strides();
                        let y_slice = yuv.y();
                        let mut y_plane = Vec::with_capacity(width * height);
                        for row in 0..height {
                            let start = row * y_stride;
                            y_plane.extend_from_slice(&y_slice[start..start + width]);
                        }
                        Ok(Some(DecodedYuv {
                            width,
                            height,
                            y_plane,
                        }))
                    }
                    Ok(None) => Ok(None),
                    Err(e) => Err(QamError::Media(format!("H264 decode error: {e}"))),
                }
            }
            VideoCodec::H265 => {
                let dec = self
                    .h265_decoder
                    .as_mut()
                    .ok_or_else(|| QamError::Media("H265 decoder not initialized".into()))?;

                for nal in rust_h265::parse_annex_b(data) {
                    dec.decode_nal(&nal)
                        .map_err(|e| QamError::Media(format!("H265 NAL decode error: {e}")))?;

                    if let Some(frame) = dec.next_output() {
                        return Ok(Some(extract_h265_y(&frame)?));
                    }
                }

                if let Some(frame) = dec.flush() {
                    return Ok(Some(extract_h265_y(&frame)?));
                }

                Ok(None)
            }
            VideoCodec::Mjpeg => {
                let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
                    .map_err(|e| QamError::Media(format!("MJPEG decode error: {e}")))?
                    .to_rgb8();
                let width = img.width() as usize;
                let height = img.height() as usize;
                let mut y_plane = Vec::with_capacity(width * height);
                for pixel in img.pixels() {
                    let [r, g, b] = pixel.0;
                    let y =
                        ((66 * (r as u32) + 129 * (g as u32) + 25 * (b as u32) + 128) >> 8) + 16;
                    y_plane.push(y.clamp(0, 255) as u8);
                }
                Ok(Some(DecodedYuv {
                    width,
                    height,
                    y_plane,
                }))
            }
        }
    }
}

fn extract_h265_y(frame: &rust_h265::decoder::Frame) -> Result<DecodedYuv, QamError> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let y_plane_raw = match &frame.y {
        rust_h265::PixelData::U8(data) => data.as_slice(),
        _ => return Err(QamError::Media("unsupported 10-bit H.265 frame".into())),
    };
    let y_plane = y_plane_raw[..width * height].to_vec();
    Ok(DecodedYuv {
        width,
        height,
        y_plane,
    })
}

fn convert_h265_frame(frame: &rust_h265::decoder::Frame) -> Result<RgbImage, QamError> {
    let width = frame.width as usize;
    let height = frame.height as usize;

    let y_plane = match &frame.y {
        rust_h265::PixelData::U8(data) => data.as_slice(),
        _ => return Err(QamError::Media("unsupported 10-bit H.265 frame".into())),
    };
    let u_plane = match &frame.u {
        rust_h265::PixelData::U8(data) => data.as_slice(),
        _ => return Err(QamError::Media("unsupported 10-bit H.265 frame".into())),
    };
    let v_plane = match &frame.v {
        rust_h265::PixelData::U8(data) => data.as_slice(),
        _ => return Err(QamError::Media("unsupported 10-bit H.265 frame".into())),
    };

    let y_stride = width;
    let u_stride = width / 2;
    let v_stride = width / 2;

    Ok(yuv420p_to_rgb(
        width, height, y_plane, y_stride, u_plane, u_stride, v_plane, v_stride,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_h264_synthetic() {
        use openh264::encoder::Encoder as H264Encoder;
        use openh264::formats::YUVBuffer;

        let width = 64;
        let height = 48;
        let mut enc = H264Encoder::new().unwrap();

        let raw = vec![180u8; width * height + 2 * (width / 2) * (height / 2)];
        let yuv = YUVBuffer::from_vec(raw, width, height);
        let bitstream = enc.encode(&yuv).unwrap().to_vec();

        let mut dec = VideoDecoder::new(VideoCodec::H264).unwrap();
        let frame = dec.decode_frame(&bitstream).unwrap();
        assert!(frame.is_some());
        let img = frame.unwrap();
        assert_eq!(img.width(), width as u32);
        assert_eq!(img.height(), height as u32);
    }
}

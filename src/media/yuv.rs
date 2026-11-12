use image::RgbImage;

/// Converts planar YUV420p data to an `image::RgbImage`.
///
/// Uses standard ITU-R BT.601 full-range conversion coefficients implemented
/// via 10-bit integer fixed-point arithmetic:
///   R = Y + 1.402 * (V - 128)
///   G = Y - 0.344136 * (U - 128) - 0.714136 * (V - 128)
///   B = Y + 1.772 * (U - 128)
#[allow(clippy::too_many_arguments)]
pub fn yuv420p_to_rgb(
    width: usize,
    height: usize,
    y_plane: &[u8],
    y_stride: usize,
    u_plane: &[u8],
    u_stride: usize,
    v_plane: &[u8],
    v_stride: usize,
) -> RgbImage {
    let mut img = RgbImage::new(width as u32, height as u32);
    let buf: &mut [u8] = img.as_mut();

    for y in 0..height {
        let y_row = y * y_stride;
        let u_row = (y / 2) * u_stride;
        let v_row = (y / 2) * v_stride;
        let out_row = y * width * 3;

        for x in 0..width {
            let y_val = y_plane[y_row + x] as i32;
            let u_val = u_plane[u_row + (x / 2)] as i32 - 128;
            let v_val = v_plane[v_row + (x / 2)] as i32 - 128;

            let r = ((y_val << 10) + 1436 * v_val + 512) >> 10;
            let g = ((y_val << 10) - 352 * u_val - 731 * v_val + 512) >> 10;
            let b = ((y_val << 10) + 1815 * u_val + 512) >> 10;

            let out_idx = out_row + x * 3;
            buf[out_idx] = r.clamp(0, 255) as u8;
            buf[out_idx + 1] = g.clamp(0, 255) as u8;
            buf[out_idx + 2] = b.clamp(0, 255) as u8;
        }
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yuv420p_to_rgb_neutrals() {
        // Solid black: Y=0, U=128, V=128 -> RGB (0, 0, 0)
        let y = vec![0u8; 4];
        let u = vec![128u8; 1];
        let v = vec![128u8; 1];
        let img = yuv420p_to_rgb(2, 2, &y, 2, &u, 1, &v, 1);
        for pixel in img.pixels() {
            assert_eq!(pixel.0, [0, 0, 0]);
        }

        // Solid white: Y=255, U=128, V=128 -> RGB (255, 255, 255)
        let y = vec![255u8; 4];
        let img = yuv420p_to_rgb(2, 2, &y, 2, &u, 1, &v, 1);
        for pixel in img.pixels() {
            assert_eq!(pixel.0, [255, 255, 255]);
        }

        // Gray: Y=128, U=128, V=128 -> RGB (128, 128, 128)
        let y = vec![128u8; 4];
        let img = yuv420p_to_rgb(2, 2, &y, 2, &u, 1, &v, 1);
        for pixel in img.pixels() {
            assert_eq!(pixel.0, [128, 128, 128]);
        }
    }
}

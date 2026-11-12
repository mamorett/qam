/// Finds all NAL units in an Annex-B stream (handling both 3-byte and 4-byte start codes).
pub fn find_nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let len = data.len();
    if len < 3 {
        return nals;
    }

    let mut start_indices = Vec::new();
    let mut i = 0;
    while i + 2 < len {
        if data[i] == 0 && data[i + 1] == 0 {
            if i + 3 < len && data[i + 2] == 0 && data[i + 3] == 1 {
                // 4-byte start code 00 00 00 01
                start_indices.push((i, i + 4));
                i += 4;
                continue;
            } else if data[i + 2] == 1 {
                // 3-byte start code 00 00 01
                start_indices.push((i, i + 3));
                i += 3;
                continue;
            }
        }
        i += 1;
    }

    for idx in 0..start_indices.len() {
        let nal_start = start_indices[idx].1;
        let nal_end = if idx + 1 < start_indices.len() {
            start_indices[idx + 1].0
        } else {
            len
        };

        if nal_start < nal_end {
            nals.push(&data[nal_start..nal_end]);
        }
    }

    nals
}

/// Converts an Annex-B stream to 4-byte length-prefixed AVCC format.
pub fn annex_b_to_avcc(data: &[u8]) -> Vec<u8> {
    let nals = find_nal_units(data);
    let mut total_len = 0;
    for n in &nals {
        total_len += 4 + n.len();
    }

    let mut avcc = Vec::with_capacity(total_len);
    for n in nals {
        let n_len = n.len() as u32;
        avcc.extend_from_slice(&n_len.to_be_bytes());
        avcc.extend_from_slice(n);
    }
    avcc
}

/// Converts an AVCC stream (4-byte big-endian length prefixed) to Annex-B (00 00 00 01 prefixed).
pub fn avcc_to_annex_b(data: &[u8]) -> Vec<u8> {
    let mut annex_b = Vec::with_capacity(data.len() + 32);
    let mut i = 0;
    while i + 4 <= data.len() {
        let nal_len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        i += 4;
        if i + nal_len > data.len() {
            break;
        }
        annex_b.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        annex_b.extend_from_slice(&data[i..i + nal_len]);
        i += nal_len;
    }
    annex_b
}

/// Checks if an H.264 NAL unit is an IDR picture (keyframe).
pub fn is_h264_keyframe(nal: &[u8]) -> bool {
    if nal.is_empty() {
        return false;
    }
    let nal_type = nal[0] & 0x1F;
    nal_type == 5 // IDR
}

/// Checks if an H.265 NAL unit is an IDR or CRA picture (keyframe / random access point).
pub fn is_h265_keyframe(nal: &[u8]) -> bool {
    if nal.is_empty() {
        return false;
    }
    let nal_type = (nal[0] >> 1) & 0x3F;
    matches!(nal_type, 19..=21) // IDR_W_RADL, IDR_N_LP, CRA_NUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annex_b_find_and_convert() {
        let mut raw = Vec::new();
        // 4-byte start code + NAL 1
        raw.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x0A]);
        // 3-byte start code + NAL 2
        raw.extend_from_slice(&[0x00, 0x00, 0x01, 0x68, 0xCE, 0x3C]);

        let nals = find_nal_units(&raw);
        assert_eq!(nals.len(), 2);
        assert_eq!(nals[0], &[0x67, 0x42, 0x0A]);
        assert_eq!(nals[1], &[0x68, 0xCE, 0x3C]);

        let avcc = annex_b_to_avcc(&raw);
        assert_eq!(avcc.len(), 4 + 3 + 4 + 3);
        assert_eq!(&avcc[0..4], &[0x00, 0x00, 0x00, 0x03]);
        assert_eq!(&avcc[4..7], &[0x67, 0x42, 0x0A]);

        let roundtrip = avcc_to_annex_b(&avcc);
        let nals_rt = find_nal_units(&roundtrip);
        assert_eq!(nals_rt, nals);
    }

    #[test]
    fn test_keyframe_identification() {
        // H.264 IDR nal_type = 5 (0x65 & 0x1F = 5)
        assert!(is_h264_keyframe(&[0x65, 0x88, 0x84]));
        // H.264 non-IDR nal_type = 1 (0x41 & 0x1F = 1)
        assert!(!is_h264_keyframe(&[0x41, 0x9A]));

        // H.265 IDR_W_RADL nal_type = 19 (0x26 >> 1 = 19)
        assert!(is_h265_keyframe(&[0x26, 0x01]));
        // H.265 CRA nal_type = 21 (0x2A >> 1 = 21)
        assert!(is_h265_keyframe(&[0x2A, 0x01]));
        // H.265 TRAIL_R nal_type = 1 (0x02 >> 1 = 1)
        assert!(!is_h265_keyframe(&[0x02, 0x01]));
    }
}

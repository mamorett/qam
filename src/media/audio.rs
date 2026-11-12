//! Standard ITU-T G.711 A-law and mu-law decoding to 16-bit signed linear PCM.

/// Decodes a single G.711 A-law byte into a 16-bit linear PCM sample.
pub fn alaw_to_linear(a: u8) -> i16 {
    let a = a ^ 0x55;
    let mut t = ((a & 0x0F) as i32) << 4;
    let seg = ((a & 0x70) >> 4) as i32;

    if seg == 0 {
        t += 8;
    } else {
        t += 0x108;
        t <<= seg - 1;
    }

    if (a & 0x80) != 0 {
        t as i16
    } else {
        (-t) as i16
    }
}

/// Decodes a single G.711 mu-law byte into a 16-bit linear PCM sample.
pub fn ulaw_to_linear(u: u8) -> i16 {
    let u = !u;
    let mut t = (((u & 0x0F) as i32) << 3) + 0x84;
    let seg = ((u & 0x70) >> 4) as i32;
    t <<= seg;
    t -= 0x84;

    if (u & 0x80) != 0 { -t as i16 } else { t as i16 }
}

/// Decodes a slice of G.711 bytes (A-law or mu-law) to 16-bit signed linear PCM samples.
pub fn decode_g711_to_s16le(data: &[u8], is_alaw: bool) -> Vec<i16> {
    let mut out = Vec::with_capacity(data.len());
    if is_alaw {
        for &b in data {
            out.push(alaw_to_linear(b));
        }
    } else {
        for &b in data {
            out.push(ulaw_to_linear(b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_g711_decode() {
        // Zero code test
        let pcm_a0 = alaw_to_linear(0xD5);
        assert_eq!(pcm_a0, 8);

        let pcm_u0 = ulaw_to_linear(0xFF);
        assert_eq!(pcm_u0, 0);

        let bytes = vec![0xD5, 0x55];
        let decoded = decode_g711_to_s16le(&bytes, true);
        assert_eq!(decoded.len(), 2);
    }
}

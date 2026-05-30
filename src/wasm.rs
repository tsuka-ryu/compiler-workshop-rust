/// Unsigned LEB128 でエンコードして bytes に追記
pub fn encode_uleb128(value: u64, bytes: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// Signed LEB128 でエンコードして bytes に追記
pub fn encode_sleb128(value: i64, bytes: &mut Vec<u8>) {
    let mut v = value;
    let mut more = true;
    while more {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        let sign_bit_set = (byte & 0x40) != 0;
        if (v == 0 && !sign_bit_set) || (v == -1 && sign_bit_set) {
            more = false;
        }
        bytes.push(if more { byte | 0x80 } else { byte });
    }
}

/// f64 を IEEE 754 (little-endian) で bytes に追記
pub fn encode_f64(value: f64, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uleb128_small() {
        let mut b = Vec::new();
        encode_uleb128(0, &mut b);
        assert_eq!(b, vec![0x00]);
    }

    #[test]
    fn uleb128_127() {
        let mut b = Vec::new();
        encode_uleb128(127, &mut b);
        assert_eq!(b, vec![0x7f]); // ちょうど7ビットに収まる最大値
    }

    #[test]
    fn uleb128_128() {
        let mut b = Vec::new();
        encode_uleb128(128, &mut b);
        assert_eq!(b, vec![0x80, 0x01]); // 7ビット超えで2バイトに
    }

    #[test]
    fn uleb128_624485() {
        // wikipedia の例：624485 → E5 8E 26
        let mut b = Vec::new();
        encode_uleb128(624485, &mut b);
        assert_eq!(b, vec![0xe5, 0x8e, 0x26]);
    }

    #[test]
    fn sleb128_positive() {
        let mut b = Vec::new();
        encode_sleb128(0, &mut b);
        assert_eq!(b, vec![0x00]);
    }

    #[test]
    fn sleb128_negative_one() {
        let mut b = Vec::new();
        encode_sleb128(-1, &mut b);
        assert_eq!(b, vec![0x7f]); // 全ビット1
    }

    #[test]
    fn sleb128_negative_large() {
        // wikipedia の例：-12345 → C7 9F 7F
        let mut b = Vec::new();
        encode_sleb128(-12345, &mut b);
        assert_eq!(b, vec![0xc7, 0x9f, 0x7f]);
    }

    #[test]
    fn f64_one() {
        let mut b = Vec::new();
        encode_f64(1.0, &mut b);
        assert_eq!(b, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f]);
    }

    #[test]
    fn f64_pi() {
        let mut b = Vec::new();
        encode_f64(std::f64::consts::PI, &mut b);
        // 一致を確かめるならRustの to_le_bytes と同じ結果
        assert_eq!(b, std::f64::consts::PI.to_le_bytes().to_vec());
    }
}

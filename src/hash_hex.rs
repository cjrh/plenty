//! Lowercase hexadecimal for cache and fingerprint bytes.
pub(crate) fn encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 15) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn preserves_leading_zeroes_and_lowercase() {
        assert_eq!(super::encode(&[0, 1, 0xab, 0xff]), "0001abff");
        assert_eq!(super::encode(&[]), "");
    }
}

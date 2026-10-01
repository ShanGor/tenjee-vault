//! 内置密码生成器：高强度随机密码（默认 ≥16 位，含大小写字母、数字、符号）。

use rand::rngs::OsRng;
use rand::seq::SliceRandom;
use rand::Rng;

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.<>?";

pub const DEFAULT_LENGTH: usize = 16;

fn all_chars() -> Vec<u8> {
    let mut all = Vec::with_capacity(LOWER.len() + UPPER.len() + DIGITS.len() + SYMBOLS.len());
    all.extend_from_slice(LOWER);
    all.extend_from_slice(UPPER);
    all.extend_from_slice(DIGITS);
    all.extend_from_slice(SYMBOLS);
    all
}

fn random_char(set: &[u8]) -> u8 {
    set[OsRng.gen_range(0..set.len())]
}

/// 生成密码：保证每类字符至少出现一次，其余从全部字符集随机填充后打乱。
pub fn generate_password(length: usize) -> String {
    let required_len = 4; // lower/upper/digit/symbol 各一
    let length = length.max(required_len);
    let all = all_chars();
    let mut chars = Vec::with_capacity(length);
    chars.push(random_char(LOWER));
    chars.push(random_char(UPPER));
    chars.push(random_char(DIGITS));
    chars.push(random_char(SYMBOLS));
    for _ in required_len..length {
        chars.push(random_char(&all));
    }
    chars.shuffle(&mut OsRng);
    String::from_utf8(chars).expect("字符集均为 ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_length_and_charset() {
        let pw = generate_password(DEFAULT_LENGTH);
        assert!(pw.len() >= 16);
        assert!(pw.bytes().any(|c| LOWER.contains(&c)));
        assert!(pw.bytes().any(|c| UPPER.contains(&c)));
        assert!(pw.bytes().any(|c| DIGITS.contains(&c)));
        assert!(pw.bytes().any(|c| SYMBOLS.contains(&c)));
    }

    #[test]
    fn passwords_are_random() {
        assert_ne!(generate_password(16), generate_password(16));
    }

    #[test]
    fn custom_length_respected() {
        assert_eq!(generate_password(24).len(), 24);
    }

    #[test]
    fn tiny_length_still_has_all_classes() {
        let pw = generate_password(1);
        assert_eq!(pw.len(), 4);
        assert!(pw.bytes().any(|c| LOWER.contains(&c)));
        assert!(pw.bytes().any(|c| UPPER.contains(&c)));
        assert!(pw.bytes().any(|c| DIGITS.contains(&c)));
        assert!(pw.bytes().any(|c| SYMBOLS.contains(&c)));
    }
}

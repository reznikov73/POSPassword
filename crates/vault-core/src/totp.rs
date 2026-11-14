//! Одноразовые коды TOTP (RFC 6238).
//!
//! Секрет хранится в записи как скрытое поле «TOTP» — либо base32-строка,
//! либо ссылка `otpauth://totp/...`. Коды считаются на этой машине,
//! наружу ничего не уходит.

use std::time::{SystemTime, UNIX_EPOCH};

use data_encoding::BASE32;
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use zeroize::Zeroizing;

use crate::{Error, Result};

/// Период по умолчанию, секунды.
pub const DEFAULT_PERIOD: u64 = 30;
/// Число цифр по умолчанию.
pub const DEFAULT_DIGITS: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotpAlgorithm {
    Sha1,
    Sha256,
    Sha512,
}

/// Разобранный секрет TOTP.
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    digits: u32,
    period: u64,
    algorithm: TotpAlgorithm,
}

impl Totp {
    /// Разбирает поле записи: ссылку `otpauth://` или голый base32.
    pub fn parse(value: &str) -> Result<Self> {
        let text = value.trim();
        if text.is_empty() {
            return Err(Error::InvalidFormat("пустой секрет TOTP".into()));
        }
        if text.to_ascii_lowercase().starts_with("otpauth://") {
            Self::parse_uri(text)
        } else {
            Self::from_base32(text)
        }
    }

    fn parse_uri(uri: &str) -> Result<Self> {
        let query = uri.split_once('?').map(|(_, query)| query).unwrap_or("");
        let mut secret = None;
        let mut digits = DEFAULT_DIGITS;
        let mut period = DEFAULT_PERIOD;
        let mut algorithm = TotpAlgorithm::Sha1;

        for pair in query.split('&') {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            match name.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(value.to_string()),
                "digits" => digits = value.parse().unwrap_or(DEFAULT_DIGITS),
                "period" => period = value.parse().unwrap_or(DEFAULT_PERIOD),
                "algorithm" => {
                    algorithm = match value.to_ascii_uppercase().as_str() {
                        "SHA256" => TotpAlgorithm::Sha256,
                        "SHA512" => TotpAlgorithm::Sha512,
                        _ => TotpAlgorithm::Sha1,
                    };
                }
                _ => {}
            }
        }

        let secret =
            secret.ok_or_else(|| Error::InvalidFormat("в ссылке otpauth нет secret".into()))?;
        let mut totp = Self::from_base32(&secret)?;
        totp.digits = digits.clamp(6, 8);
        totp.period = period.clamp(1, 300);
        totp.algorithm = algorithm;
        Ok(totp)
    }

    fn from_base32(value: &str) -> Result<Self> {
        let cleaned: String = value
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '-')
            .collect::<String>()
            .to_ascii_uppercase();

        let unpadded = cleaned.trim_end_matches('=').to_string();
        if unpadded.is_empty() {
            return Err(Error::InvalidFormat("пустой секрет TOTP".into()));
        }

        let mut padded = unpadded;
        while !padded.len().is_multiple_of(8) {
            padded.push('=');
        }

        let bytes = BASE32
            .decode(padded.as_bytes())
            .map_err(|_| Error::InvalidFormat("секрет TOTP не является base32".into()))?;

        Ok(Self {
            secret: Zeroizing::new(bytes),
            digits: DEFAULT_DIGITS,
            period: DEFAULT_PERIOD,
            algorithm: TotpAlgorithm::Sha1,
        })
    }

    pub fn digits(&self) -> u32 {
        self.digits
    }

    pub fn period(&self) -> u64 {
        self.period
    }

    /// Код на момент времени `unix_seconds`.
    pub fn code_at(&self, unix_seconds: u64) -> String {
        let counter = unix_seconds / self.period;
        let message = counter.to_be_bytes();
        let digest = match self.algorithm {
            TotpAlgorithm::Sha1 => hmac_sha1(&self.secret, &message),
            TotpAlgorithm::Sha256 => hmac_sha256(&self.secret, &message),
            TotpAlgorithm::Sha512 => hmac_sha512(&self.secret, &message),
        };

        let offset = (digest[digest.len() - 1] & 0x0f) as usize;
        let binary = (u32::from(digest[offset]) & 0x7f) << 24
            | u32::from(digest[offset + 1]) << 16
            | u32::from(digest[offset + 2]) << 8
            | u32::from(digest[offset + 3]);
        let code = binary % 10u32.pow(self.digits);
        format!("{code:0width$}", width = self.digits as usize)
    }

    /// Код на текущий момент.
    pub fn code_now(&self) -> String {
        self.code_at(now_secs())
    }

    /// Сколько секунд осталось до смены кода.
    pub fn seconds_left(&self, unix_seconds: u64) -> u64 {
        self.period - (unix_seconds % self.period)
    }
}

impl std::fmt::Debug for Totp {
    // Секрет в отладочный вывод не попадает.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Totp")
            .field("digits", &self.digits)
            .field("period", &self.period)
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

/// Текущее время в секундах UNIX.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC принимает ключ любой длины");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC принимает ключ любой длины");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha512(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha512>::new_from_slice(key).expect("HMAC принимает ключ любой длины");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_encoding::BASE32;

    #[test]
    fn rfc6238_sha256_vector() {
        let secret = BASE32.encode(b"12345678901234567890123456789012");
        let mut totp = Totp::from_base32(&secret).expect("секрет");
        totp.digits = 8;
        totp.algorithm = TotpAlgorithm::Sha256;
        assert_eq!(totp.code_at(59), "46119246");
    }

    #[test]
    fn rfc6238_sha512_vector() {
        let secret =
            BASE32.encode(b"1234567890123456789012345678901234567890123456789012345678901234");
        let mut totp = Totp::from_base32(&secret).expect("секрет");
        totp.digits = 8;
        totp.algorithm = TotpAlgorithm::Sha512;
        assert_eq!(totp.code_at(59), "90693936");
    }
}

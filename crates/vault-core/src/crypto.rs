use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use zeroize::Zeroizing;

use crate::{Error, Result};

pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
pub const KEY_LEN: usize = 32;

fn cipher_for(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new_from_slice(key).expect("длина ключа закреплена типом")
}

fn nonce_of(nonce: &[u8; NONCE_LEN]) -> XNonce {
    XNonce::try_from(&nonce[..]).expect("длина nonce закреплена типом")
}

/// Параметры Argon2id, которые записываются в заголовок файла, чтобы
/// хранилище можно было открыть с теми же настройками после обновления.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// Стоимость по памяти в КиБ.
    pub m_cost: u32,
    /// Число итераций.
    pub t_cost: u32,
    /// Степень параллелизма.
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        // 64 МиБ, 3 итерации, 1 поток: ~0.3 с на десктопе.
        Self {
            m_cost: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

impl KdfParams {
    /// Проверяет параметры, прочитанные из файла, до передачи их в Argon2.
    /// Нижняя граница защищает от подмены заголовка на заведомо слабые значения.
    pub fn validate(&self) -> Result<()> {
        if self.m_cost < 8 * 1024 {
            return Err(Error::InvalidFormat(format!(
                "слишком низкая стоимость по памяти: {} КиБ",
                self.m_cost
            )));
        }
        if self.t_cost == 0 || self.p_cost == 0 || self.p_cost > 64 {
            return Err(Error::InvalidFormat("недопустимые параметры Argon2".into()));
        }
        Ok(())
    }
}

/// Выводит ключ шифрования из мастер-пароля.
pub fn derive_key(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    params.validate()?;
    let argon_params = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|e| Error::Kdf(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, salt, &mut key[..])
        .map_err(|e| Error::Kdf(e.to_string()))?;
    Ok(key)
}

/// Шифрует данные. `aad` — заголовок файла, он аутентифицируется, но не шифруется.
pub fn seal(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    let cipher = cipher_for(key);
    cipher
        .encrypt(
            &nonce_of(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Crypto("не удалось зашифровать".into()))
}

/// Расшифровывает данные. Любое расхождение AAD или тега даёт одну ошибку.
pub fn open(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let cipher = cipher_for(key);
    let plaintext = cipher
        .decrypt(
            &nonce_of(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| Error::InvalidPasswordOrCorrupt)?;
    Ok(Zeroizing::new(plaintext))
}

/// Заполняет буфер случайными байтами из ОС.
pub fn random_bytes(buf: &mut [u8]) {
    rand::fill(buf);
}

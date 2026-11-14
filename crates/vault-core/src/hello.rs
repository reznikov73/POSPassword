//! Обёртка ключа хранилища для разблокировки через Windows Hello.
//!
//! Рядом с файлом хранилища появляется сайдкар `<файл>.hello`:
//!
//! ```text
//! magic(8) | version(2) | nonce(12) | wrapped_key(48) | dpapi_blob(..)
//! ```
//!
//! Ключ хранилища шифруется AES-256-GCM случайным 32-байтным ключом обёртки,
//! а ключ обёртки приложение защищает DPAPI и передаёт в `dpapi_blob` — ядро
//! считает его непрозрачными данными. Формат основного файла не меняется:
//! сайдкар можно удалить в любой момент без потери данных.

use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::consts::U12;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use zeroize::Zeroizing;

use crate::crypto::{KEY_LEN, random_bytes};
use crate::vault::write_atomic;
use crate::{Error, Result};

const MAGIC: &[u8; 8] = b"OPHELLO1";
const VERSION: u16 = 1;

/// Nonce AES-GCM — 96 бит.
const NONCE_LEN: usize = 12;
/// Шифротекст ключа хранилища: 32 байта ключа + 16 байт тега GCM.
const WRAPPED_LEN: usize = KEY_LEN + 16;
/// Магия, версия и nonce — начало файла и одновременно AAD.
const PREFIX_LEN: usize = 8 + 2 + NONCE_LEN;
const MIN_FILE_LEN: usize = PREFIX_LEN + WRAPPED_LEN;

/// Путь сайдкара: `<путь-хранилища>.hello`.
pub fn sidecar_path(vault_path: &Path) -> PathBuf {
    let mut name = vault_path.as_os_str().to_os_string();
    name.push(".hello");
    PathBuf::from(name)
}

/// Случайный ключ обёртки. Зануляется при уничтожении.
pub fn generate_wrapping_key() -> Zeroizing<[u8; KEY_LEN]> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    random_bytes(key.as_mut());
    key
}

/// DPAPI-блоб ключа обёртки из сайдкара.
///
/// Приложение снимает с него защиту DPAPI и получает ключ обёртки для
/// [`crate::Vault::open_with_hello`]. Ядро блоб не интерпретирует.
pub fn protected_wrapping_key(vault_path: &Path) -> Result<Vec<u8>> {
    Ok(HelloEnvelope::read(&sidecar_path(vault_path))?.dpapi_blob)
}

/// Содержимое сайдкара.
pub(crate) struct HelloEnvelope {
    nonce: [u8; NONCE_LEN],
    wrapped: [u8; WRAPPED_LEN],
    /// Ключ обёртки под защитой DPAPI. Непрозрачен для ядра.
    dpapi_blob: Vec<u8>,
}

impl HelloEnvelope {
    /// Шифрует ключ хранилища ключом обёртки. Nonce свежий при каждой записи.
    pub(crate) fn seal(
        vault_key: &[u8; KEY_LEN],
        wrapping_key: &[u8; KEY_LEN],
        dpapi_blob: Vec<u8>,
    ) -> Self {
        let mut nonce = [0u8; NONCE_LEN];
        random_bytes(&mut nonce);

        let cipher = Aes256Gcm::new_from_slice(wrapping_key).expect("длина ключа закреплена типом");
        let wrapped = cipher
            .encrypt(
                &nonce_of(&nonce),
                Payload {
                    msg: vault_key,
                    aad: &prefix_aad(&nonce),
                },
            )
            .expect("шифрование ключа корректной длины не падает");

        let mut wrapped_key = [0u8; WRAPPED_LEN];
        wrapped_key.copy_from_slice(&wrapped);

        Self {
            nonce,
            wrapped: wrapped_key,
            dpapi_blob,
        }
    }

    /// Разворачивает ключ хранилища. Повреждение сайдкара и неверный ключ
    /// обёртки неотличимы — единая ошибка, как в основном формате.
    pub(crate) fn open(&self, wrapping_key: &[u8; KEY_LEN]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let cipher = Aes256Gcm::new_from_slice(wrapping_key).expect("длина ключа закреплена типом");
        let plaintext = cipher
            .decrypt(
                &nonce_of(&self.nonce),
                Payload {
                    msg: &self.wrapped,
                    aad: &prefix_aad(&self.nonce),
                },
            )
            .map_err(|_| Error::HelloUnwrapFailed)?;

        let mut vault_key = Zeroizing::new([0u8; KEY_LEN]);
        vault_key.copy_from_slice(&plaintext);
        Ok(vault_key)
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MIN_FILE_LEN + self.dpapi_blob.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.wrapped);
        out.extend_from_slice(&self.dpapi_blob);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < MIN_FILE_LEN {
            return Err(Error::InvalidFormat(
                "сайдкар Windows Hello короче заголовка".into(),
            ));
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            return Err(Error::InvalidFormat(
                "сайдкар Windows Hello: неверная сигнатура".into(),
            ));
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }

        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[10..10 + NONCE_LEN]);
        let mut wrapped = [0u8; WRAPPED_LEN];
        wrapped.copy_from_slice(&bytes[22..22 + WRAPPED_LEN]);
        let dpapi_blob = bytes[MIN_FILE_LEN..].to_vec();

        Ok(Self {
            nonce,
            wrapped,
            dpapi_blob,
        })
    }

    /// Читает и разбирает сайдкар.
    pub(crate) fn read(path: &Path) -> Result<Self> {
        Self::decode(&fs::read(path)?)
    }

    /// Пишет сайдкар атомарно, как и основное хранилище.
    pub(crate) fn write(&self, path: &Path) -> Result<()> {
        write_atomic(path, &self.encode())
    }
}

fn nonce_of(nonce: &[u8; NONCE_LEN]) -> Nonce<U12> {
    Nonce::try_from(&nonce[..]).expect("длина nonce закреплена типом")
}

/// Префикс файла (магия, версия, nonce) — он же AAD для AES-GCM.
fn prefix_aad(nonce: &[u8; NONCE_LEN]) -> [u8; PREFIX_LEN] {
    let mut aad = [0u8; PREFIX_LEN];
    aad[..MAGIC.len()].copy_from_slice(MAGIC);
    aad[MAGIC.len()..MAGIC.len() + 2].copy_from_slice(&VERSION.to_le_bytes());
    aad[MAGIC.len() + 2..].copy_from_slice(nonce);
    aad
}

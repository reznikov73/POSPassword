//! Ядро локального менеджера паролей.
//!
//! Формат файла-хранилища (v1):
//!
//! ```text
//! magic(8) | version(2) | kdf_id(1) | m_cost(4) | t_cost(4) | p_cost(4)
//!          | salt(16) | nonce(24) | ciphertext(..)
//! ```
//!
//! Ключ выводится из мастер-пароля через Argon2id, полезная нагрузка шифруется
//! XChaCha20-Poly1305. Заголовок целиком передаётся как AAD, поэтому его нельзя
//! подменить, не сломав аутентификацию.

mod audit;
mod crypto;
mod describe;
mod error;
mod export;
mod generator;
mod hello;
mod import;
mod lock;
mod model;
mod passphrase;
mod psl;
mod quickadd;
mod sites;
mod totp;
mod vault;

pub use audit::{
    AuditReport, Expiry, Finding, audit, expiries, is_common_password, is_expiry_label,
    parse_date,
};
pub use crypto::{KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN};
pub use describe::auto_description;
pub use error::{Error, Result};
pub use export::{ExportFormat, export_csv, export_json};
pub use generator::{GeneratorOptions, generate_password, password_entropy_bits};
pub use hello::{generate_wrapping_key, protected_wrapping_key, sidecar_path};
pub use import::{ImportFormat, ImportSummary, import_file};
pub use lock::{FileLock, lock_path};
pub use model::{Field, Item, ItemKind, PASSWORD_HISTORY_MAX, PasswordVersion, VaultData};
pub use passphrase::{
    PassphraseOptions, Separator, Wordlist, generate_passphrase, passphrase_entropy_bits,
};
pub use quickadd::{ParsedCredentials, parse_credentials};
pub use sites::{
    MatchStrength, normalize_host, registrable_domain, url_matches, url_matches_title,
};
pub use totp::{Totp, now_secs};
pub use vault::{FORMAT_VERSION, KdfPreset, MAGIC, Vault, backup_path};

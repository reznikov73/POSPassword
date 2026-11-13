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

mod crypto;
mod error;
mod generator;
mod import;
mod model;
mod vault;

pub use crypto::{KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN};
pub use error::{Error, Result};
pub use generator::{GeneratorOptions, generate_password};
pub use import::{ImportFormat, ImportSummary, import_file};
pub use model::{Field, Item, ItemKind, VaultData};
pub use vault::{FORMAT_VERSION, MAGIC, Vault};

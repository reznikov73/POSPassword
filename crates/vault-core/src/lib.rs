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
mod model;
mod vault;

pub use crypto::{KdfParams, KEY_LEN, NONCE_LEN, SALT_LEN};
pub use error::{Error, Result};
pub use generator::{generate_password, GeneratorOptions};
pub use model::{Item, ItemKind, VaultData};
pub use vault::{Vault, FORMAT_VERSION, MAGIC};

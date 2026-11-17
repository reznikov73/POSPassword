use std::path::PathBuf;

/// Ошибки ядра. Тексты намеренно не раскрывают, что именно не совпало:
/// при неверном пароле и при повреждённом файле возвращается одна и та же
/// ошибка `InvalidPasswordOrCorrupt`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("неверный пароль или повреждённый файл хранилища")]
    InvalidPasswordOrCorrupt,

    #[error("файл не является хранилищем этого формата: {0}")]
    InvalidFormat(String),

    #[error("неподдерживаемая версия формата: {0}")]
    UnsupportedVersion(u16),

    #[error("ошибка вывода ключа: {0}")]
    Kdf(String),

    #[error("ошибка шифрования: {0}")]
    Crypto(String),

    #[error("обёртка Windows Hello повреждена или ключ обёртки не подходит")]
    HelloUnwrapFailed,

    #[error("ошибка сериализации: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("ошибка CSV: {0}")]
    Csv(#[from] csv::Error),

    #[error("запись не найдена: {0}")]
    ItemNotFound(String),

    #[error("хранилище заблокировано")]
    Locked,

    #[error("хранилище занято другим процессом: {0}")]
    Busy(String),

    #[error("некорректный путь: {0}")]
    BadPath(PathBuf),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

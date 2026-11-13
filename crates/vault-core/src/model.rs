use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

/// Тип записи. Пока поддерживаются логины и защищённые заметки;
/// карты добавим отдельным этапом, чтобы не тащить лишние поля в v1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    #[default]
    Login,
    SecureNote,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Login => "Логин",
            ItemKind::SecureNote => "Заметка",
        }
    }
}

/// Одна запись хранилища. Секретные поля зануляются при уничтожении структуры.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: Uuid,
    pub kind: ItemKind,
    pub title: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Item {
    pub fn new(kind: ItemKind, title: impl Into<String>) -> Self {
        let now = now_secs();
        Self {
            id: Uuid::new_v4(),
            kind,
            title: title.into(),
            username: String::new(),
            password: String::new(),
            url: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_secs();
    }

    /// Поиск по всем непустым полям, кроме собственно пароля:
    /// искать по паролю — значит подтверждать его наличие.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        self.title.to_lowercase().contains(&q)
            || self.username.to_lowercase().contains(&q)
            || self.url.to_lowercase().contains(&q)
            || self.notes.to_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        self.password.zeroize();
        self.notes.zeroize();
    }
}

/// Содержимое расшифрованного хранилища.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultData {
    pub schema: u32,
    #[serde(default)]
    pub items: Vec<Item>,
}

impl Default for VaultData {
    fn default() -> Self {
        Self {
            schema: 1,
            items: Vec::new(),
        }
    }
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

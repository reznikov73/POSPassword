use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

/// Тип записи. Набор расширяется, но старые значения в файле остаются валидными.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    #[default]
    Login,
    SecureNote,
    Card,
    BankAccount,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Login => "Логин",
            ItemKind::SecureNote => "Заметка",
            ItemKind::Card => "Карта",
            ItemKind::BankAccount => "Счёт",
        }
    }

    pub fn all() -> [ItemKind; 4] {
        [
            ItemKind::Login,
            ItemKind::SecureNote,
            ItemKind::Card,
            ItemKind::BankAccount,
        ]
    }
}

/// Дополнительное поле записи: то, что не влезает в логин/пароль/сайт.
/// `sensitive` помечает значения, которые нельзя искать и которые нужно маскировать.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub sensitive: bool,
}

impl Field {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            sensitive: false,
        }
    }

    pub fn secret(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            sensitive: true,
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
    /// Поля карт, счетов и произвольные поля из импорта.
    #[serde(default)]
    pub fields: Vec<Field>,
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
            fields: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Карта: заготовка полей, чтобы в форме сразу было что заполнять.
    pub fn new_card(title: impl Into<String>) -> Self {
        let mut item = Self::new(ItemKind::Card, title);
        item.fields = vec![
            Field::new("Владелец", ""),
            Field::secret("Номер карты", ""),
            Field::new("Срок действия", ""),
            Field::secret("CVV", ""),
            Field::new("Платёжная система", ""),
            Field::new("Банк", ""),
        ];
        item
    }

    /// Банковский счёт: IBAN, SWIFT и прочее.
    pub fn new_bank_account(title: impl Into<String>) -> Self {
        let mut item = Self::new(ItemKind::BankAccount, title);
        item.fields = vec![
            Field::new("Банк", ""),
            Field::secret("Номер счёта", ""),
            Field::secret("IBAN", ""),
            Field::new("SWIFT/BIC", ""),
            Field::new("Валюта", ""),
        ];
        item
    }

    pub fn touch(&mut self) {
        self.updated_at = now_secs();
    }

    pub fn field(&self, label: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.label == label)
    }

    pub fn set_field(&mut self, label: &str, value: impl Into<String>) {
        let value = value.into();
        match self.fields.iter_mut().find(|f| f.label == label) {
            Some(field) => field.value = value,
            None => self.fields.push(Field::new(label, value)),
        }
    }

    /// Поиск по видимым полям. Секретные значения (пароль, номер карты, CVV)
    /// намеренно не индексируются: иначе по выдаче можно подтверждать догадки.
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
            || self
                .fields
                .iter()
                .filter(|f| !f.sensitive)
                .any(|f| f.value.to_lowercase().contains(&q))
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        self.password.zeroize();
        self.notes.zeroize();
        for field in &mut self.fields {
            if field.sensitive {
                field.value.zeroize();
            }
        }
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
            schema: 2,
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

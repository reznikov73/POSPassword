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
    Passport,
    ForeignPassport,
    Snils,
    Inn,
    DriverLicense,
    MedicalPolicy,
    DigitalSignature,
    ApiKey,
    Wifi,
    Pin,
    Insurance,
    Property,
    Health,
    Vehicle,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Login => "Логин",
            ItemKind::SecureNote => "Заметка",
            ItemKind::Card => "Карта",
            ItemKind::BankAccount => "Счёт",
            ItemKind::Passport => "Паспорт РФ",
            ItemKind::ForeignPassport => "Загранпаспорт",
            ItemKind::Snils => "СНИЛС",
            ItemKind::Inn => "ИНН",
            ItemKind::DriverLicense => "Водительское",
            ItemKind::MedicalPolicy => "Полис ОМС",
            ItemKind::DigitalSignature => "ЭЦП",
            ItemKind::ApiKey => "API-ключ",
            ItemKind::Wifi => "Wi-Fi",
            ItemKind::Pin => "PIN-код",
            ItemKind::Insurance => "Страховка",
            ItemKind::Property => "Недвижимость",
            ItemKind::Health => "Медицина",
            ItemKind::Vehicle => "Автомобиль",
        }
    }

    pub fn all() -> [ItemKind; 18] {
        [
            ItemKind::Login,
            ItemKind::SecureNote,
            ItemKind::Card,
            ItemKind::BankAccount,
            ItemKind::Passport,
            ItemKind::ForeignPassport,
            ItemKind::Snils,
            ItemKind::Inn,
            ItemKind::DriverLicense,
            ItemKind::MedicalPolicy,
            ItemKind::DigitalSignature,
            ItemKind::ApiKey,
            ItemKind::Wifi,
            ItemKind::Pin,
            ItemKind::Insurance,
            ItemKind::Property,
            ItemKind::Health,
            ItemKind::Vehicle,
        ]
    }

    /// Готовый набор полей для типа записи. Пустой — только базовые поля.
    pub fn template(self) -> Vec<Field> {
        let plain = Field::new;
        let secret = Field::secret;

        match self {
            ItemKind::Login | ItemKind::SecureNote => Vec::new(),

            ItemKind::Card => vec![
                plain("Владелец", ""),
                secret("Номер карты", ""),
                plain("Срок действия", ""),
                secret("CVV", ""),
                plain("Платёжная система", ""),
                plain("Банк", ""),
            ],

            ItemKind::BankAccount => vec![
                plain("Банк", ""),
                secret("Номер счёта", ""),
                secret("IBAN", ""),
                plain("SWIFT/BIC", ""),
                plain("Валюта", ""),
            ],

            ItemKind::Passport => vec![
                secret("Серия и номер", ""),
                plain("Кем выдан", ""),
                plain("Дата выдачи", ""),
                plain("Код подразделения", ""),
                plain("Прописка", ""),
            ],

            ItemKind::ForeignPassport => vec![
                secret("Серия и номер", ""),
                plain("Кем выдан", ""),
                plain("Дата выдачи", ""),
                plain("Срок действия", ""),
            ],

            ItemKind::Snils => vec![secret("Номер СНИЛС", "")],

            ItemKind::Inn => vec![
                secret("Номер ИНН", ""),
                plain("Налоговый орган", ""),
                plain("Дата постановки на учёт", ""),
            ],

            ItemKind::DriverLicense => vec![
                secret("Серия и номер", ""),
                plain("Категории", ""),
                plain("Дата выдачи", ""),
                plain("Срок действия", ""),
            ],

            ItemKind::MedicalPolicy => vec![
                secret("Номер полиса", ""),
                plain("Страховая компания", ""),
                plain("Срок действия", ""),
            ],

            ItemKind::DigitalSignature => vec![
                plain("Владелец", ""),
                plain("Носитель", ""),
                secret("PIN-код", ""),
                plain("Удостоверяющий центр", ""),
                plain("Срок действия", ""),
            ],

            ItemKind::ApiKey => vec![
                plain("Сервис", ""),
                secret("Ключ", ""),
                plain("Права доступа", ""),
                plain("Срок действия", ""),
            ],

            ItemKind::Wifi => vec![
                plain("Сеть (SSID)", ""),
                secret("Пароль", ""),
                plain("Тип шифрования", ""),
                plain("Где находится", ""),
            ],

            ItemKind::Pin => vec![
                plain("Назначение", ""),
                secret("PIN", ""),
                secret("PUK", ""),
                plain("Примечание", ""),
            ],

            ItemKind::Insurance => vec![
                plain("Страховая компания", ""),
                secret("Номер полиса", ""),
                plain("Вид страхования", ""),
                plain("Срок действия", ""),
                plain("Телефон", ""),
            ],

            ItemKind::Property => vec![
                plain("Адрес", ""),
                plain("Кадастровый номер", ""),
                plain("Площадь", ""),
                plain("Тип документа", ""),
                plain("Номер документа", ""),
                plain("Собственник", ""),
            ],

            ItemKind::Health => vec![
                plain("Группа крови", ""),
                plain("Аллергии", ""),
                plain("Хронические болезни", ""),
                plain("Лекарства", ""),
                plain("Поликлиника", ""),
                plain("Врач", ""),
            ],

            ItemKind::Vehicle => vec![
                plain("Марка и модель", ""),
                plain("VIN", ""),
                plain("Госномер", ""),
                secret("Номер СТС", ""),
                secret("Номер ПТС", ""),
                plain("ОСАГО до", ""),
                plain("Техосмотр до", ""),
            ],
        }
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
    /// Дополнительные сайты записи: один сервис часто живёт на нескольких
    /// адресах (логин, кабинет, форум). Основной `url` — для совместимости
    /// со старыми файлами; поиск и матчинг идут по всем адресам сразу.
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Поля карт, документов, счетов и произвольные поля из импорта.
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
            urls: Vec::new(),
            notes: String::new(),
            tags: Vec::new(),
            fields: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Запись с готовым набором полей для своего типа.
    pub fn with_template(kind: ItemKind, title: impl Into<String>) -> Self {
        let mut item = Self::new(kind, title);
        item.fields = kind.template();
        item
    }

    pub fn new_card(title: impl Into<String>) -> Self {
        Self::with_template(ItemKind::Card, title)
    }

    pub fn new_bank_account(title: impl Into<String>) -> Self {
        Self::with_template(ItemKind::BankAccount, title)
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

    /// Все сайты записи: основной плюс дополнительные, без пустых строк.
    pub fn all_urls(&self) -> Vec<&str> {
        let mut urls: Vec<&str> = Vec::new();
        if !self.url.trim().is_empty() {
            urls.push(self.url.trim());
        }
        urls.extend(self.urls.iter().map(|u| u.trim()).filter(|u| !u.is_empty()));
        urls.sort_unstable();
        urls.dedup();
        urls
    }

    /// Относится ли запись к адресу: основной или дополнительный сайт
    /// совпадает по базовому домену (`lk.domclick.ru` и `domclick.ru` — один).
    pub fn matches_url(&self, url: &str) -> bool {
        self.all_urls()
            .iter()
            .any(|item| crate::sites::url_matches(item, url))
    }

    /// Насколько заголовок окна похож на сайт записи — для подсказки автоввода.
    pub fn url_match_strength(&self, title: &str) -> crate::sites::MatchStrength {
        self.all_urls()
            .iter()
            .map(|url| crate::sites::url_matches_title(url, title))
            .max()
            .unwrap_or(crate::sites::MatchStrength::None)
    }

    /// Поиск по видимым полям. Секретные значения (пароль, номер карты, CVV,
    /// номера документов) намеренно не индексируются: иначе по выдаче можно
    /// подтверждать догадки.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        self.title.to_lowercase().contains(&q)
            || self.username.to_lowercase().contains(&q)
            || self.url.to_lowercase().contains(&q)
            || self.urls.iter().any(|u| u.to_lowercase().contains(&q))
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
            schema: 3,
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

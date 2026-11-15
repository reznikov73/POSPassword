//! Осмысленные описания записей.
//!
//! Описание собирается из названия, логина, сайта и открытых полей записи и
//! пишется в заметки. Секретные значения (пароли, номера карт, документов,
//! ключи) в описание не попадают: заметка индексируется поиском и всегда
//! видна на экране, а скрытые поля скрыты именно поэтому.

use time::OffsetDateTime;

use crate::model::{Item, ItemKind};

/// Составляет человекочитаемое описание записи.
///
/// Примеры: «Логин на сайте mail.ru: ivan. Подключён одноразовый код (TOTP).
/// Обновлено 12.05.2026.», «Карта банка Тинькофф, номер •••• 1234, действует
/// до 12/28. Обновлено 03.02.2026.».
pub fn auto_description(item: &Item) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Открытое поле записи по подписи: скрытые значения в описание не берём.
    let plain = |label: &str| {
        item.field(label)
            .filter(|field| !field.sensitive)
            .map(|field| field.value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    // Скрытое поле — только его «хвост» (последние 4 знака), как на карте.
    let secret_tail = |label: &str| {
        item.field(label)
            .map(|field| field.value.trim())
            .filter(|value| value.chars().count() >= 4)
            .map(|value| {
                let tail: String = value
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                format!("•••• {tail}")
            })
    };

    match item.kind {
        ItemKind::Login => {
            let site = domain_of(&item.url);
            match (&site, item.username.trim()) {
                (Some(site), user) if !user.is_empty() => {
                    parts.push(format!("Логин на сайте {site}: {user}."));
                }
                (Some(site), _) => parts.push(format!("Вход на сайт {site}.")),
                (None, user) if !user.is_empty() => {
                    parts.push(format!("Логин {user}."));
                }
                (None, _) => parts.push("Логин для сайта.".to_string()),
            }
            if has_totp(item) {
                parts.push("Подключён одноразовый код (TOTP).".to_string());
            }
        }
        ItemKind::SecureNote => parts.push("Личная заметка.".to_string()),
        ItemKind::Card => {
            let mut text = String::from("Карта");
            if let Some(bank) = plain("Банк") {
                text.push_str(&format!(" банка {bank}"));
            } else if let Some(system) = plain("Платёжная система") {
                text.push_str(&format!(" ({system})"));
            }
            text.push(':');
            parts.push(text);
            if let Some(number) = secret_tail("Номер карты") {
                parts.push(format!("номер {number},"));
            }
            if let Some(expiry) = plain("Срок действия") {
                parts.push(format!("действует до {expiry}."));
            }
        }
        ItemKind::BankAccount => {
            let bank = plain("Банк").unwrap_or_else(|| "банка".to_string());
            let mut text = format!("Счёт в {bank}");
            if let Some(currency) = plain("Валюта") {
                text.push_str(&format!(", валюта {currency}"));
            }
            text.push_str(". Номер счёта — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::Passport => {
            let mut text = String::from("Паспорт РФ");
            if let Some(issued) = plain("Кем выдан") {
                text.push_str(&format!(", выдан: {issued}"));
            }
            if let Some(date) = plain("Дата выдачи") {
                text.push_str(&format!(" ({date})"));
            }
            text.push_str(". Серия и номер — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::ForeignPassport => {
            let mut text = String::from("Загранпаспорт");
            if let Some(expiry) = plain("Срок действия") {
                text.push_str(&format!(", действует до {expiry}"));
            }
            text.push_str(". Номер — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::Snils => {
            parts.push("Страховой номер СНИЛС (номер — в скрытом поле).".to_string())
        }
        ItemKind::Inn => {
            let mut text = String::from("ИНН");
            if let Some(authority) = plain("Налоговый орган") {
                text.push_str(&format!(", налоговый орган: {authority}"));
            }
            text.push_str(". Номер — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::DriverLicense => {
            let mut text = String::from("Водительское удостоверение");
            if let Some(categories) = plain("Категории") {
                text.push_str(&format!(", категории {categories}"));
            }
            text.push_str(". Номер — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::MedicalPolicy => {
            let mut text = String::from("Полис ОМС");
            if let Some(company) = plain("Страховая компания") {
                text.push_str(&format!(" компании «{company}»"));
            }
            if let Some(expiry) = plain("Срок действия") {
                text.push_str(&format!(", действует до {expiry}"));
            }
            text.push_str(". Номер — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::DigitalSignature => {
            let mut text = String::from("Электронная подпись");
            if let Some(medium) = plain("Носитель") {
                text.push_str(&format!(" на носителе «{medium}»"));
            }
            if let Some(center) = plain("Удостоверяющий центр") {
                text.push_str(&format!(", удостоверяющий центр: {center}"));
            }
            text.push('.');
            parts.push(text);
        }
        ItemKind::ApiKey => {
            let mut text = String::from("API-ключ");
            if let Some(service) = plain("Сервис") {
                text.push_str(&format!(" для сервиса «{service}»"));
            }
            if let Some(rights) = plain("Права доступа") {
                text.push_str(&format!(", права: {rights}"));
            }
            text.push_str(". Сам ключ — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::Wifi => {
            let mut text = String::from("Wi-Fi");
            if let Some(ssid) = plain("Сеть (SSID)") {
                text.push_str(&format!(" «{ssid}»"));
            }
            if let Some(place) = plain("Где находится") {
                text.push_str(&format!(", находится: {place}"));
            }
            if let Some(encryption) = plain("Тип шифрования") {
                text.push_str(&format!(", шифрование {encryption}"));
            }
            text.push_str(". Пароль — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::Pin => {
            let mut text = String::from("PIN-код");
            if let Some(purpose) = plain("Назначение") {
                text.push_str(&format!(" для «{purpose}»"));
            }
            text.push_str(". Код — в скрытом поле.");
            parts.push(text);
        }
        ItemKind::Insurance => {
            let mut text = String::from("Страховка");
            if let Some(company) = plain("Страховая компания") {
                text.push_str(&format!(" компании «{company}»"));
            }
            if let Some(kind) = plain("Вид страхования") {
                text.push_str(&format!(", вид: {kind}"));
            }
            if let Some(expiry) = plain("Срок действия") {
                text.push_str(&format!(", действует до {expiry}"));
            }
            text.push('.');
            parts.push(text);
        }
        ItemKind::Property => {
            let mut text = String::from("Недвижимость");
            if let Some(address) = plain("Адрес") {
                text.push_str(&format!(" по адресу {address}"));
            }
            if let Some(area) = plain("Площадь") {
                text.push_str(&format!(", площадь {area}"));
            }
            text.push('.');
            parts.push(text);
        }
        ItemKind::Health => {
            let mut text = String::from("Медицина");
            if let Some(blood) = plain("Группа крови") {
                text.push_str(&format!(" (группа крови {blood})"));
            }
            if let Some(clinic) = plain("Поликлиника") {
                text.push_str(&format!(", поликлиника: {clinic}"));
            }
            text.push('.');
            parts.push(text);
        }
        ItemKind::Vehicle => {
            let mut text = String::from("Автомобиль");
            if let Some(model) = plain("Марка и модель") {
                text.push_str(&format!(" {model}"));
            }
            if let Some(plate) = plain("Госномер") {
                text.push_str(&format!(", номер {plate}"));
            }
            text.push_str(", документы — в скрытых полях.");
            parts.push(text);
        }
    }

    if !item.tags.is_empty() {
        parts.push(format!("Теги: {}.", item.tags.join(", ")));
    }
    parts.push(format!("Обновлено {}.", format_date(item.updated_at)));

    parts.join(" ")
}

/// Есть ли в записи поле с одноразовым кодом (TOTP).
fn has_totp(item: &Item) -> bool {
    item.fields.iter().any(|field| {
        field.label.to_ascii_lowercase().contains("totp")
            || field
                .value
                .trim()
                .to_ascii_lowercase()
                .starts_with("otpauth://")
    })
}

/// Домен из адреса без схемы, «www.» и пути.
fn domain_of(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_start_matches("www.");
    (!host.trim().is_empty()).then(|| host.to_string())
}

/// Дата в формате ДД.ММ.ГГГГ из Unix-времени (по UTC).
pub(crate) fn format_date(secs: i64) -> String {
    let moment = OffsetDateTime::from_unix_timestamp(secs).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:02}.{:02}.{}",
        moment.day(),
        moment.month() as u8,
        moment.year()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Field;

    #[test]
    fn login_description_mentions_site_and_login() {
        let mut item = Item::new(ItemKind::Login, "Почта");
        item.username = "ivan".to_string();
        item.url = "https://mail.ru".to_string();
        let text = auto_description(&item);
        assert!(text.contains("mail.ru"), "в описании нет сайта: {text}");
        assert!(text.contains("ivan"), "в описании нет логина: {text}");
        assert!(!text.contains("Обновлено ."), "пустая дата: {text}");
    }

    #[test]
    fn description_never_leaks_passwords() {
        let mut item = Item::with_template(ItemKind::Card, "Карта");
        item.set_field("Владелец", "Иван Петров");
        item.set_field("Номер карты", "4111111111111234");
        item.set_field("CVV", "123");
        item.set_field("Срок действия", "12/28");
        item.set_field("Банк", "Наш Банк");
        item.password = "secret-password".to_string();
        let text = auto_description(&item);
        assert!(
            !text.contains("4111111111111234"),
            "полный номер утёк: {text}"
        );
        assert!(!text.contains("4111111"), "начало номера утёкло: {text}");
        assert!(text.contains("•••• 1234"), "нет маски номера: {text}");
        assert!(!text.contains("secret-password"));
        assert!(text.contains("Наш Банк"));
    }

    #[test]
    fn every_kind_gets_nonempty_description() {
        for kind in ItemKind::all() {
            let item = Item::with_template(kind, "Запись");
            let text = auto_description(&item);
            assert!(
                !text.trim().is_empty() && text.contains("Обновлено"),
                "у типа «{}» пустое описание: «{text}»",
                kind.label()
            );
        }
    }

    #[test]
    fn date_is_formatted_correctly() {
        // 1 января 1970, 00:00 UTC.
        assert_eq!(format_date(0), "01.01.1970");
        // 5 мая 2025, 12:00 UTC = 1746446400.
        assert_eq!(format_date(1_746_446_400), "05.05.2025");
    }

    #[test]
    fn totp_is_mentioned() {
        let mut item = Item::new(ItemKind::Login, "Сервис");
        item.fields.push(Field::secret("TOTP", "JBSWY3DPEHPK3PXP"));
        let text = auto_description(&item);
        assert!(text.contains("TOTP"), "нет упоминания TOTP: {text}");
    }

    #[test]
    fn tags_are_listed() {
        let mut item = Item::new(ItemKind::SecureNote, "Заметка");
        item.tags = vec!["работа".to_string(), "важное".to_string()];
        let text = auto_description(&item);
        assert!(text.contains("Теги: работа, важное."), "нет тегов: {text}");
    }
}

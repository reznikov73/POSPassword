//! Импорт из других менеджеров паролей.
//!
//! Поддерживаются:
//! - Bitwarden: экспорт `.json` (незашифрованный) и `.csv`;
//! - 1Password: `.csv`;
//! - обычный CSV с колонками name/url/username/password.
//!
//! Импорт никогда не перезаписывает существующие записи: совпадения по названию
//! и логину (или номеру карты) считаются дубликатами и пропускаются.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::model::{Field, Item, ItemKind};
use crate::{Error, Result, Vault};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImportFormat {
    /// Определить по расширению и заголовкам файла.
    #[default]
    Auto,
    BitwardenJson,
    BitwardenCsv,
    OnePasswordCsv,
    GenericCsv,
}

impl ImportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ImportFormat::Auto => "Определить автоматически",
            ImportFormat::BitwardenJson => "Bitwarden (JSON)",
            ImportFormat::BitwardenCsv => "Bitwarden (CSV)",
            ImportFormat::OnePasswordCsv => "1Password (CSV)",
            ImportFormat::GenericCsv => "Обычный CSV",
        }
    }

    pub fn all() -> [ImportFormat; 5] {
        [
            ImportFormat::Auto,
            ImportFormat::BitwardenJson,
            ImportFormat::BitwardenCsv,
            ImportFormat::OnePasswordCsv,
            ImportFormat::GenericCsv,
        ]
    }
}

#[derive(Debug, Default, Clone)]
pub struct ImportSummary {
    pub added: usize,
    pub duplicates: usize,
    pub unsupported: usize,
    pub warnings: Vec<String>,
}

/// Читает файл и добавляет записи в хранилище. Сохраняет файл один раз в конце.
pub fn import_file(
    vault: &mut Vault,
    path: impl AsRef<Path>,
    format: ImportFormat,
) -> Result<ImportSummary> {
    let path = path.as_ref();
    let data = fs::read(path)?;

    let format = match format {
        ImportFormat::Auto => detect_format(path, &data)?,
        explicit => explicit,
    };

    let parsed = match format {
        ImportFormat::BitwardenJson => parse_bitwarden_json(&data)?,
        ImportFormat::BitwardenCsv => parse_csv_like(&data, CsvFlavor::Bitwarden)?,
        ImportFormat::OnePasswordCsv => parse_csv_like(&data, CsvFlavor::OnePassword)?,
        ImportFormat::GenericCsv => parse_csv_like(&data, CsvFlavor::Generic)?,
        ImportFormat::Auto => unreachable!("формат определён выше"),
    };

    let mut summary = ImportSummary {
        unsupported: parsed.unsupported,
        warnings: parsed.warnings,
        ..Default::default()
    };

    let existing: Vec<(String, String)> = vault.items().iter().map(dedup_key).collect();
    let mut to_add = Vec::new();
    let mut known = existing;

    for item in parsed.items {
        let key = dedup_key(&item);
        if known.contains(&key) {
            summary.duplicates += 1;
            continue;
        }
        known.push(key);
        to_add.push(item);
    }

    summary.added = to_add.len();
    vault.add_many(to_add)?;

    Ok(summary)
}

/// Ключ для поиска дубликатов: название + логин (или номер карты / IBAN).
fn dedup_key(item: &Item) -> (String, String) {
    let second = match item.kind {
        ItemKind::Card => item
            .field("Номер карты")
            .map(|f| f.value.clone())
            .unwrap_or_default(),
        ItemKind::BankAccount => item
            .field("IBAN")
            .map(|f| f.value.clone())
            .unwrap_or_default(),
        _ => item.username.clone(),
    };
    (
        item.title.trim().to_lowercase(),
        second.trim().to_lowercase(),
    )
}

fn detect_format(path: &Path, data: &[u8]) -> Result<ImportFormat> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "json" => Ok(ImportFormat::BitwardenJson),
        "csv" | "txt" => {
            let header = String::from_utf8_lossy(data)
                .lines()
                .next()
                .unwrap_or("")
                .to_lowercase();
            if header.contains("login_password") {
                Ok(ImportFormat::BitwardenCsv)
            } else if header.contains("title") && header.contains("username") {
                Ok(ImportFormat::OnePasswordCsv)
            } else if header.contains("name") || header.contains("url") {
                Ok(ImportFormat::GenericCsv)
            } else {
                Err(Error::InvalidFormat(
                    "не удалось определить формат CSV по заголовку".into(),
                ))
            }
        }
        _ => Err(Error::InvalidFormat(
            "поддерживаются файлы .json и .csv".into(),
        )),
    }
}

struct Parsed {
    items: Vec<Item>,
    warnings: Vec<String>,
    unsupported: usize,
}

// ---------------------------------------------------------------- Bitwarden JSON

fn parse_bitwarden_json(data: &[u8]) -> Result<Parsed> {
    let root: Value = serde_json::from_slice(data)?;

    if root
        .get("encrypted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(Error::InvalidFormat(
            "файл зашифрован: в Bitwarden выберите экспорт .json без пароля".into(),
        ));
    }

    let entries = root
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::InvalidFormat("в JSON нет массива items".into()))?;

    let mut parsed = Parsed {
        items: Vec::new(),
        warnings: Vec::new(),
        unsupported: 0,
    };

    for entry in entries {
        let kind = entry.get("type").and_then(Value::as_u64).unwrap_or(0);
        let name = text(entry, "name");
        let title = if name.trim().is_empty() {
            "Без названия".to_string()
        } else {
            name
        };
        let notes = text(entry, "notes");

        match kind {
            1 => {
                let login = entry.get("login").cloned().unwrap_or(Value::Null);
                let mut item = Item::new(ItemKind::Login, title);
                item.username = text(&login, "username");
                item.password = text(&login, "password");
                item.url = login
                    .get("uris")
                    .and_then(Value::as_array)
                    .and_then(|u| u.first())
                    .map(|u| text(u, "uri"))
                    .unwrap_or_default();
                item.notes = notes;
                let totp = text(&login, "totp");
                if !totp.is_empty() {
                    item.fields.push(Field::secret("TOTP", totp));
                }
                push_custom_fields(&mut item, entry);
                parsed.items.push(item);
            }
            2 => {
                let mut item = Item::new(ItemKind::SecureNote, title);
                item.notes = notes;
                push_custom_fields(&mut item, entry);
                parsed.items.push(item);
            }
            3 => {
                let card = entry.get("card").cloned().unwrap_or(Value::Null);
                let mut item = Item::new(ItemKind::Card, title);
                item.notes = notes;

                let holder = text(&card, "cardholderName");
                if !holder.is_empty() {
                    item.fields.push(Field::new("Владелец", holder));
                }
                let number = text(&card, "number");
                if !number.is_empty() {
                    item.fields.push(Field::secret("Номер карты", number));
                }
                let expiry = expiry_string(&text(&card, "expMonth"), &text(&card, "expYear"));
                if !expiry.is_empty() {
                    item.fields.push(Field::new("Срок действия", expiry));
                }
                let code = text(&card, "code");
                if !code.is_empty() {
                    item.fields.push(Field::secret("CVV", code));
                }
                let brand = text(&card, "brand");
                if !brand.is_empty() {
                    item.fields.push(Field::new("Платёжная система", brand));
                }
                push_custom_fields(&mut item, entry);
                parsed.items.push(item);
            }
            4 => {
                // Удостоверение личности переносим как заметку с полями.
                let mut item = Item::new(ItemKind::SecureNote, title);
                item.notes = notes;
                let identity = entry.get("identity").cloned().unwrap_or(Value::Null);
                for (key, label) in [
                    ("firstName", "Имя"),
                    ("lastName", "Фамилия"),
                    ("email", "Почта"),
                    ("phone", "Телефон"),
                    ("address1", "Адрес"),
                ] {
                    let value = text(&identity, key);
                    if !value.is_empty() {
                        item.fields.push(Field::new(label, value));
                    }
                }
                push_custom_fields(&mut item, entry);
                parsed.items.push(item);
                parsed
                    .warnings
                    .push("записи типа «удостоверение» перенесены как заметки".to_string());
            }
            _ => parsed.unsupported += 1,
        }
    }

    Ok(parsed)
}

fn push_custom_fields(item: &mut Item, entry: &Value) {
    let Some(fields) = entry.get("fields").and_then(Value::as_array) else {
        return;
    };
    for field in fields {
        let label = text(field, "name");
        let value = text(field, "value");
        if label.trim().is_empty() || value.is_empty() {
            continue;
        }
        // type 1 в Bitwarden — скрытое поле.
        let hidden = field.get("type").and_then(Value::as_u64) == Some(1);
        item.fields.push(if hidden {
            Field::secret(label, value)
        } else {
            Field::new(label, value)
        });
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn expiry_string(month: &str, year: &str) -> String {
    if month.is_empty() && year.is_empty() {
        return String::new();
    }
    let month_number: u32 = month.parse().unwrap_or(0);
    let short_year = if year.len() == 4 {
        year[2..].to_string()
    } else {
        year.to_string()
    };
    format!("{month_number:02}/{short_year}")
}

// ---------------------------------------------------------------- CSV

#[derive(Clone, Copy, PartialEq, Eq)]
enum CsvFlavor {
    Bitwarden,
    OnePassword,
    Generic,
}

fn parse_csv_like(data: &[u8], flavor: CsvFlavor) -> Result<Parsed> {
    let content = decode_text(data);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| Error::InvalidFormat(format!("не удалось прочитать заголовок CSV: {e}")))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();

    let index = |name: &str| headers.iter().position(|h| h == name);
    let mut parsed = Parsed {
        items: Vec::new(),
        warnings: Vec::new(),
        unsupported: 0,
    };

    for record in reader.records() {
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                parsed.warnings.push(format!("строка пропущена: {err}"));
                continue;
            }
        };
        let get = |name: &str| -> String {
            index(name)
                .and_then(|i| record.get(i))
                .unwrap_or("")
                .trim()
                .to_string()
        };

        match flavor {
            CsvFlavor::Bitwarden => {
                let name = get("name");
                if name.is_empty() {
                    continue;
                }
                let notes = get("notes");
                let item = match get("type").to_lowercase().as_str() {
                    "note" => {
                        let mut item = Item::new(ItemKind::SecureNote, name);
                        item.notes = notes;
                        item
                    }
                    "card" => {
                        let mut item = Item::new(ItemKind::Card, name);
                        item.notes = notes;
                        item
                    }
                    "identity" => {
                        parsed.unsupported += 1;
                        continue;
                    }
                    _ => {
                        let mut item = Item::new(ItemKind::Login, name);
                        item.username = get("login_username");
                        item.password = get("login_password");
                        item.url = get("login_uri");
                        item.notes = notes;
                        let totp = get("login_totp");
                        if !totp.is_empty() {
                            item.fields.push(Field::secret("TOTP", totp));
                        }
                        item
                    }
                };

                let mut item = item;
                for line in get("fields").lines() {
                    if let Some((label, value)) = line.split_once(':') {
                        let label = label.trim();
                        let value = value.trim();
                        if !label.is_empty() && !value.is_empty() {
                            item.fields.push(Field::new(label, value));
                        }
                    }
                }
                parsed.items.push(item);
            }
            CsvFlavor::OnePassword => {
                let title = get("title");
                if title.is_empty() {
                    continue;
                }
                let mut item = Item::new(ItemKind::Login, title);
                item.username = get("username");
                item.password = get("password");
                item.url = get("url");
                item.notes = get("notes");
                let totp = get("otpauth");
                if !totp.is_empty() {
                    item.fields.push(Field::secret("TOTP", totp));
                }
                let tags = get("tags");
                if !tags.is_empty() {
                    item.tags = tags
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
                parsed.items.push(item);
            }
            CsvFlavor::Generic => {
                let title = first_non_empty(&[get("name"), get("title")]);
                if title.is_empty() {
                    continue;
                }
                let mut item = Item::new(ItemKind::Login, title);
                item.username = first_non_empty(&[get("username"), get("login")]);
                item.password = first_non_empty(&[get("password"), get("pass")]);
                item.url = first_non_empty(&[get("url"), get("website")]);
                item.notes = get("notes");
                parsed.items.push(item);
            }
        }
    }

    if parsed.items.is_empty() {
        return Err(Error::InvalidFormat(
            "в файле не нашлось ни одной записи — проверьте колонки".into(),
        ));
    }

    Ok(parsed)
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|v| !v.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// CSV из Windows часто в UTF-8 с BOM или в UTF-16 (такое делает Excel).
fn decode_text(data: &[u8]) -> String {
    if data.starts_with(&[0xFF, 0xFE]) {
        let (pairs, _remainder) = data[2..].as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        return String::from_utf16_lossy(&units);
    }
    let text = String::from_utf8_lossy(data).to_string();
    text.trim_start_matches('\u{feff}').to_string()
}

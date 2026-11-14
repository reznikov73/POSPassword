//! Экспорт хранилища: JSON с полным сохранением типов записей и CSV
//! в раскладке Bitwarden, которую читают и другие менеджеры.
//!
//! Экспорт — это открытый текст: файл содержит все пароли. Вызывающий код
//! обязан предупредить пользователя и удалить файл после переноса.

use serde::Serialize;

use crate::model::{Item, ItemKind};
use crate::{Error, Result};

/// Колонки CSV в порядке Bitwarden.
const CSV_HEADERS: [&str; 11] = [
    "folder",
    "favorite",
    "type",
    "name",
    "notes",
    "fields",
    "reprompt",
    "login_uri",
    "login_username",
    "login_password",
    "login_totp",
];

/// Формат экспорта.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    /// Полный JSON: типы записей, поля, теги, даты.
    Json,
    /// CSV в раскладке Bitwarden: логины, заметки, карты.
    Csv,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Json => "JSON (полный)",
            ExportFormat::Csv => "CSV (Bitwarden)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
        }
    }

    pub fn all() -> [ExportFormat; 2] {
        [ExportFormat::Json, ExportFormat::Csv]
    }
}

#[derive(Serialize)]
struct JsonExport<'a> {
    format: &'static str,
    version: u32,
    exported_at: i64,
    items: &'a [Item],
}

/// Полный экспорт в JSON. Все типы и поля сохраняются как есть.
pub fn export_json(items: &[Item], exported_at: i64) -> Result<String> {
    let document = JsonExport {
        format: "hranitel",
        version: 1,
        exported_at,
        items,
    };
    Ok(serde_json::to_string_pretty(&document)?)
}

/// Экспорт в CSV. Документы и ключи уходят как «login» с полями в колонке
/// `fields` — так их прочитает и Bitwarden, и наш собственный импорт.
pub fn export_csv(items: &[Item]) -> Result<String> {
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record(CSV_HEADERS)?;

    for item in items {
        let totp = item
            .fields
            .iter()
            .find(|field| is_totp_field(&field.label, &field.value))
            .map(|field| field.value.clone())
            .unwrap_or_default();

        let fields = item
            .fields
            .iter()
            .filter(|field| !is_totp_field(&field.label, &field.value))
            .map(|field| format!("{}: {}", field.label, field.value))
            .collect::<Vec<_>>()
            .join("\n");

        writer.write_record([
            String::new(),
            "0".to_string(),
            csv_kind(item.kind).to_string(),
            item.title.clone(),
            item.notes.clone(),
            fields,
            "0".to_string(),
            item.url.clone(),
            item.username.clone(),
            item.password.clone(),
            totp,
        ])?;
    }

    let bytes = writer
        .into_inner()
        .map_err(|err| Error::Csv(err.into_error().into()))?;
    String::from_utf8(bytes).map_err(|err| Error::InvalidFormat(format!("CSV не в UTF-8: {err}")))
}

fn csv_kind(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::SecureNote => "note",
        ItemKind::Card => "card",
        _ => "login",
    }
}

fn is_totp_field(label: &str, value: &str) -> bool {
    label.to_ascii_lowercase().contains("totp")
        || value.trim().to_ascii_lowercase().starts_with("otpauth://")
}

use std::fs;

use vault_core::{
    ExportFormat, Field, ImportFormat, Item, ItemKind, Vault, export_csv, export_json, import_file,
};

const PASSWORD: &str = "correct horse battery";

fn login() -> Item {
    let mut item = Item::new(ItemKind::Login, "GitHub");
    item.username = "octocat".into();
    item.password = "s3cret-password".into();
    item.url = "https://github.com".into();
    item.notes = "заметка".into();
    item.fields.push(Field::secret("TOTP", "GEZDGNBVGY3TQOJQ"));
    item
}

fn note() -> Item {
    let mut item = Item::new(ItemKind::SecureNote, "Коды восстановления");
    item.notes = "1-2-3-4".into();
    item
}

fn card() -> Item {
    let mut item = Item::with_template(ItemKind::Card, "Карта");
    item.set_field("Владелец", "ИВАН ИВАНОВ");
    item
}

#[test]
fn formats_have_labels_and_extensions() {
    assert_eq!(ExportFormat::Json.extension(), "json");
    assert_eq!(ExportFormat::Csv.extension(), "csv");
    assert_eq!(ExportFormat::all().len(), 2);
}

#[test]
fn json_export_contains_items() {
    let text = export_json(&[login(), note()], 1_700_000_000).expect("экспорт");
    let value: serde_json::Value = serde_json::from_str(&text).expect("валидный JSON");

    assert_eq!(value["format"], "hranitel");
    assert_eq!(value["version"], 1);
    assert_eq!(value["items"].as_array().expect("массив").len(), 2);
    assert_eq!(value["items"][0]["title"], "GitHub");
    assert_eq!(value["items"][0]["password"], "s3cret-password");
}

#[test]
fn csv_export_roundtrips_through_import() {
    let dir = tempfile::tempdir().expect("каталог");
    let csv = export_csv(&[login(), note(), card()]).expect("экспорт");
    assert!(csv.contains("login_password"));
    assert!(csv.contains("octocat"));

    let export_path = dir.path().join("export.csv");
    fs::write(&export_path, &csv).expect("запись экспорта");

    let vault_path = dir.path().join("vault.opvault");
    let mut vault = Vault::create(&vault_path, PASSWORD).expect("хранилище");
    let summary =
        import_file(&mut vault, &export_path, ImportFormat::BitwardenCsv).expect("импорт");

    assert_eq!(summary.added, 3);
    assert_eq!(vault.items().len(), 3);

    let github = vault
        .items()
        .iter()
        .find(|item| item.title == "GitHub")
        .expect("логин вернулся");
    assert_eq!(github.username, "octocat");
    assert_eq!(github.password, "s3cret-password");
    assert_eq!(github.url, "https://github.com");
    assert!(
        github
            .fields
            .iter()
            .any(|field| field.label == "TOTP" && field.value == "GEZDGNBVGY3TQOJQ"),
        "секрет TOTP должен вернуться в записи"
    );

    let saved_note = vault
        .items()
        .iter()
        .find(|item| item.title == "Коды восстановления")
        .expect("заметка вернулась");
    assert_eq!(saved_note.kind, ItemKind::SecureNote);
    assert_eq!(saved_note.notes, "1-2-3-4");
}

#[test]
fn csv_export_escapes_special_characters() {
    let mut item = login();
    item.notes = "строка с \"кавычками\" и, запятой\nвторая строка".into();
    item.title = "Название, с запятой".into();

    let csv = export_csv(&[item]).expect("экспорт");
    let dir = tempfile::tempdir().expect("каталог");
    let path = dir.path().join("export.csv");
    fs::write(&path, &csv).expect("запись");

    let mut vault = Vault::create(dir.path().join("v.opvault"), PASSWORD).expect("хранилище");
    import_file(&mut vault, &path, ImportFormat::BitwardenCsv).expect("импорт");
    let item = &vault.items()[0];
    assert_eq!(item.title, "Название, с запятой");
    assert!(item.notes.contains("кавычками"));
}

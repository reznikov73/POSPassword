use std::fs;

use vault_core::{Field, ImportFormat, Item, ItemKind, Vault, import_file};

const PASSWORD: &str = "correct horse battery";

fn setup() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("временный каталог");
    let path = dir.path().join("vault.opvault");
    let vault = Vault::create(&path, PASSWORD).expect("создание хранилища");
    (dir, vault)
}

const BITWARDEN_JSON: &str = r#"
{
  "encrypted": false,
  "items": [
    {
      "type": 1,
      "name": "GitHub",
      "notes": "рабочий аккаунт",
      "login": {
        "username": "octocat",
        "password": "s3cret",
        "totp": "otpauth://totp/GitHub?secret=ABC",
        "uris": [{ "uri": "https://github.com" }]
      },
      "fields": [{ "name": "Пин-код", "value": "1234", "type": 1 }]
    },
    {
      "type": 3,
      "name": "Visa",
      "notes": "",
      "card": {
        "cardholderName": "IVAN PETROV",
        "brand": "Visa",
        "number": "4111111111111111",
        "expMonth": "3",
        "expYear": "2030",
        "code": "123"
      }
    },
    { "type": 2, "name": "Заметка", "notes": "текст заметки" }
  ]
}
"#;

#[test]
fn imports_bitwarden_json() {
    let (dir, mut vault) = setup();
    let file = dir.path().join("bitwarden_export.json");
    fs::write(&file, BITWARDEN_JSON).expect("файл");

    let summary = import_file(&mut vault, &file, ImportFormat::Auto).expect("импорт");
    assert_eq!(summary.added, 3, "три записи");
    assert_eq!(summary.duplicates, 0);

    let github = vault
        .items()
        .iter()
        .find(|i| i.title == "GitHub")
        .expect("логин");
    assert_eq!(github.username, "octocat");
    assert_eq!(github.password, "s3cret");
    assert_eq!(github.url, "https://github.com");
    assert_eq!(github.notes, "рабочий аккаунт");

    let totp = github.field("TOTP").expect("totp перенесён");
    assert!(totp.sensitive, "totp должен быть скрытым");
    let pin = github.field("Пин-код").expect("произвольное поле");
    assert!(pin.sensitive, "скрытое поле Bitwarden остаётся скрытым");

    let card = vault
        .items()
        .iter()
        .find(|i| i.title == "Visa")
        .expect("карта");
    assert_eq!(card.kind, ItemKind::Card);
    assert_eq!(card.field("Владелец").unwrap().value, "IVAN PETROV");
    assert_eq!(
        card.field("Срок действия").unwrap().value,
        "03/30",
        "месяц с ведущим нулём, год из двух цифр"
    );
    let number = card.field("Номер карты").expect("номер");
    assert_eq!(number.value, "4111111111111111");
    assert!(number.sensitive, "номер карты скрытый");

    let note = vault
        .items()
        .iter()
        .find(|i| i.title == "Заметка")
        .expect("заметка");
    assert_eq!(note.kind, ItemKind::SecureNote);
    assert_eq!(note.notes, "текст заметки");
}

#[test]
fn repeated_import_skips_duplicates() {
    let (dir, mut vault) = setup();
    let file = dir.path().join("bitwarden_export.json");
    fs::write(&file, BITWARDEN_JSON).expect("файл");

    import_file(&mut vault, &file, ImportFormat::Auto).expect("первый импорт");
    let before = vault.items().len();

    let summary = import_file(&mut vault, &file, ImportFormat::Auto).expect("второй импорт");
    assert_eq!(summary.added, 0, "ничего не добавлено");
    assert_eq!(summary.duplicates, 3, "все три записи — дубликаты");
    assert_eq!(vault.items().len(), before, "количество не изменилось");
}

#[test]
fn imports_bitwarden_csv() {
    let (dir, mut vault) = setup();
    let file = dir.path().join("bitwarden.csv");
    fs::write(
        &file,
        "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
         ,0,login,Почта,,,0,https://mail.example.com,me@example.com,pass123,\n",
    )
    .expect("файл");

    let summary = import_file(&mut vault, &file, ImportFormat::Auto).expect("импорт");
    assert_eq!(summary.added, 1);

    let item = &vault.items()[0];
    assert_eq!(item.title, "Почта");
    assert_eq!(item.username, "me@example.com");
    assert_eq!(item.password, "pass123");
    assert_eq!(item.url, "https://mail.example.com");
}

#[test]
fn imports_1password_csv() {
    let (dir, mut vault) = setup();
    let file = dir.path().join("1password.csv");
    fs::write(
        &file,
        "Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\n\
         Банк,https://bank.example.com,client,secret,otpauth://totp/x?secret=Y,false,false,финансы,заметка\n",
    )
    .expect("файл");

    let summary = import_file(&mut vault, &file, ImportFormat::Auto).expect("импорт");
    assert_eq!(summary.added, 1);

    let item = &vault.items()[0];
    assert_eq!(item.title, "Банк");
    assert_eq!(item.tags, vec!["финансы".to_string()]);
    assert!(item.field("TOTP").is_some());
}

#[test]
fn encrypted_bitwarden_json_is_refused() {
    let (dir, mut vault) = setup();
    let file = dir.path().join("encrypted.json");
    fs::write(&file, r#"{"encrypted": true, "items": []}"#).expect("файл");

    let err = import_file(&mut vault, &file, ImportFormat::Auto).expect_err("должна быть ошибка");
    assert!(err.to_string().contains("зашифрован"));
}

#[test]
fn unknown_file_type_is_refused() {
    let (dir, mut vault) = setup();
    let file = dir.path().join("vault.txt");
    fs::write(&file, "просто текст").expect("файл");

    assert!(import_file(&mut vault, &file, ImportFormat::Auto).is_err());
}

#[test]
fn card_secrets_are_not_searchable() {
    let (_dir, mut vault) = setup();
    let mut card = Item::new_card("Visa");
    card.set_field("Номер карты", "4111111111111111");
    card.set_field("Владелец", "IVAN PETROV");
    vault.add(card).expect("карта добавлена");

    assert_eq!(
        vault.search("4111").len(),
        0,
        "номер карты не индексируется"
    );
    assert_eq!(vault.search("PETROV").len(), 1, "владелец ищется");
    assert_eq!(vault.search("visa").len(), 1, "название ищется");
}

#[test]
fn card_templates_have_fields() {
    let card = Item::new_card("Карта");
    assert_eq!(card.kind, ItemKind::Card);
    assert!(card.fields.len() >= 5);
    assert!(card.field("CVV").expect("cvv").sensitive);

    let account = Item::new_bank_account("Счёт");
    assert_eq!(account.kind, ItemKind::BankAccount);
    assert!(account.field("IBAN").expect("iban").sensitive);
}

#[test]
fn sensitive_fields_are_masked_on_drop() {
    // Проверяем, что тип вообще поддерживает скрытые поля и их можно создать.
    let field = Field::secret("CVV", "123");
    assert!(field.sensitive);
    assert_eq!(field.value, "123");

    let plain = Field::new("Банк", "Тинькофф");
    assert!(!plain.sensitive);
}

#[test]
fn yandex_csv_is_imported() {
    let (dir, mut vault) = setup();
    let csv = "\u{feff}url,username,password,comment,tags\r\n\
https://www.yandex.ru/,user1,pass1,заметка,работа\r\n\
https://mail.yandex.ru/inbox,user2,pass2,,\r\n\
https://github.com/login,user1,pass1,,\r\n";
    let path = dir.path().join("yandex.csv");
    fs::write(&path, csv).expect("запись файла");

    let summary = import_file(&mut vault, &path, ImportFormat::Auto).expect("импорт");
    assert_eq!(summary.added, 3);
    assert_eq!(summary.unsupported, 0);

    let titles: Vec<String> = vault
        .items()
        .iter()
        .map(|item| item.title.clone())
        .collect();
    assert!(titles.contains(&"yandex.ru".to_string()));
    assert!(titles.contains(&"mail.yandex.ru".to_string()));
    assert!(titles.contains(&"github.com".to_string()));

    let yandex = vault
        .items()
        .iter()
        .find(|item| item.title == "yandex.ru")
        .expect("запись Яндекса");
    assert_eq!(yandex.kind, ItemKind::Login);
    assert_eq!(yandex.username, "user1");
    assert_eq!(yandex.password, "pass1");
    assert_eq!(yandex.notes, "заметка");
    assert_eq!(yandex.tags, vec!["работа"]);
}

#[test]
fn yandex_csv_skips_duplicates_and_empty_rows() {
    let (dir, mut vault) = setup();
    let csv = "url,username,password,comment,tags\r\n\
https://example.com/login,user,pass,,\r\n\
,,,,\r\n";
    let path = dir.path().join("yandex.csv");
    fs::write(&path, csv).expect("запись файла");

    let first = import_file(&mut vault, &path, ImportFormat::YandexCsv).expect("импорт");
    assert_eq!(first.added, 1);
    assert_eq!(first.unsupported, 1);

    let second = import_file(&mut vault, &path, ImportFormat::YandexCsv).expect("повторный импорт");
    assert_eq!(second.added, 0);
    assert_eq!(second.duplicates, 1);
}

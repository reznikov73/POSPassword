//! Golden-тесты совместимости формата (Т-3 из ТЗ 0.13.0).
//!
//! Фикстуры в `tests/fixtures` созданы версией 0.12.3 (schema 3) и
//! заморожены. Тесты проверяют: новая версия открывает старый файл,
//! читает все данные, сохраняет и открывает снова без потерь —
//! по данным; побайтового равенства нет (schema 4, свежий nonce).
//!
//! Каждый тест работает с собственной копией фикстуры во временном
//! каталоге: так параллельные тесты не спорят за блокировку файла
//! (Ф-6) и не оставляют сайдкар-локи рядом с фикстурами.

use std::path::PathBuf;
use vault_core::{Item, Vault};

const PASSWORD: &str = "golden-master-0123";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Копирует фикстуру во временный каталог и открывает копию.
fn open_copy(name: &str, dir: &tempfile::TempDir) -> Vault {
    let source = fixture(name);
    assert!(source.exists(), "нет фикстуры {name}");
    let copy = dir.path().join(name);
    std::fs::copy(&source, &copy).unwrap();
    Vault::open(&copy, PASSWORD).expect("открыть golden-файл 0.12.3")
}

fn snapshot(items: &[Item]) -> Vec<Item> {
    items.to_vec()
}

fn roundtrip(name: &str, expected_items: usize) {
    let dir = tempfile::tempdir().unwrap();

    // Открыть копию, прочитать.
    let vault = open_copy(name, &dir);
    let before = snapshot(vault.items());
    assert_eq!(before.len(), expected_items, "число записей в {name}");

    // Сохранить и открыть снова: схема обновляется до 4, данные — те же.
    vault.save().expect("сохранить копию новым кодом");
    drop(vault);
    let reopened = Vault::open(dir.path().join(name), PASSWORD).expect("открыть сохранённое");
    let after = snapshot(reopened.items());

    assert_eq!(
        after.len(),
        expected_items,
        "число записей после сохранения"
    );
    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!(a.id, b.id, "id записи");
        assert_eq!(a.kind, b.kind, "тип записи");
        assert_eq!(a.title, b.title, "название");
        assert_eq!(a.username, b.username, "логин");
        assert_eq!(a.password, b.password, "пароль");
        assert_eq!(a.url, b.url, "основной сайт");
        assert_eq!(a.urls, b.urls, "дополнительные сайты");
        assert_eq!(a.favorite, b.favorite, "избранное");
        assert!(
            a.password_history.is_empty() && b.password_history.is_empty(),
            "история пароля в фикстурах 0.12.x должна быть пуста"
        );
        assert_eq!(a.notes, b.notes, "заметки");
        assert_eq!(a.tags, b.tags, "теги");
        assert_eq!(a.fields, b.fields, "поля");
        assert_eq!(a.created_at, b.created_at, "дата создания");
        assert_eq!(a.updated_at, b.updated_at, "дата обновления");
    }
}

#[test]
fn golden_empty_vault_roundtrip() {
    roundtrip("golden_empty_0123.opvault", 0);
}

#[test]
fn golden_logins_roundtrip() {
    roundtrip("golden_logins_0123.opvault", 3);
}

#[test]
fn golden_all_kinds_roundtrip() {
    roundtrip("golden_all_kinds_0123.opvault", 19);
}

/// Проверка содержимого логинов: TOTP-поле, urls и теги читаются.
#[test]
fn golden_logins_content() {
    let dir = tempfile::tempdir().unwrap();
    let vault = open_copy("golden_logins_0123.opvault", &dir);
    let github = vault
        .items()
        .iter()
        .find(|i| i.title == "GitHub")
        .expect("запись GitHub");
    assert_eq!(github.urls.len(), 2);
    assert!(
        github
            .fields
            .iter()
            .any(|f| f.label == "TOTP" && f.sensitive)
    );
    assert!(github.tags.contains(&"разработка".to_string()));
    drop(vault);

    let dir2 = tempfile::tempdir().unwrap();
    let card_vault = open_copy("golden_all_kinds_0123.opvault", &dir2);
    let card = card_vault
        .items()
        .iter()
        .find(|i| i.title == "Карта Тинькофф")
        .expect("карта");
    assert!(
        card.fields
            .iter()
            .any(|f| f.label == "Срок действия" && f.value == "12/27")
    );
}

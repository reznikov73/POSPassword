//! Генератор golden-фикстур хранилищ для тестов совместимости.
//!
//! ЗАМОРОЖЕНО: фикстуры в tests/fixtures созданы кодом 0.12.3 (schema 3)
//! и намеренно НЕ перегенерируются. Новый запуск этого примера создаст
//! файлы уже schema 4 — для регенерации под новую версию смените имена.
//!
//! Запуск: `cargo run -p vault-core --example gen_golden` — пишет файлы в
//! `tests/fixtures/` рядом с исходниками крейта.

use vault_core::{Field, Item, ItemKind, Vault};

const PASSWORD: &str = "golden-master-0123";

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fill(mut item: Item, username: &str, password: &str, url: &str) -> Item {
    item.username = username.into();
    item.password = password.into();
    item.url = url.into();
    item
}

fn main() {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).expect("создать каталог fixtures");

    // 1. Пустое хранилище.
    let empty = Vault::create(dir.join("golden_empty_0123.opvault"), PASSWORD).unwrap();
    empty.save().unwrap();

    // 2. Логины: несколько сайтов, теги, TOTP, чувствительные поля.
    let mut logins = Vault::create(dir.join("golden_logins_0123.opvault"), PASSWORD).unwrap();
    let mut github = fill(
        Item::new(ItemKind::Login, "GitHub"),
        "octocat",
        "s3cret-GH-2026",
        "https://github.com",
    );
    github.urls = vec![
        "https://gist.github.com".into(),
        "https://api.github.com".into(),
    ];
    github.tags = vec!["разработка".into(), "работа".into()];
    github.notes = "рабочий аккаунт".into();
    github
        .fields
        .push(Field::secret("TOTP", "JBSWY3DPEHPK3PXP"));
    github.fields.push(Field::new("Команда", "core-team"));
    logins.add(github).unwrap();

    let mut gos = fill(
        Item::new(ItemKind::Login, "Госуслуги"),
        "+7 921 000-00-00",
        "Gos!Parol#2026",
        "https://gosuslugi.ru",
    );
    gos.urls = vec!["https://lk.gosuslugi.ru".into()];
    gos.tags = vec!["государство".into()];
    gos.fields.push(Field::secret("Кодовое слово", "котик"));
    logins.add(gos).unwrap();

    let mut mail = fill(
        Item::new(ItemKind::Login, "mail.ru"),
        "ivan@mail.ru",
        "OldPass-2024",
        "https://mail.ru",
    );
    mail.notes = "почта".into();
    logins.add(mail).unwrap();
    logins.save().unwrap();

    // 3. Все 18 типов с заполненными шаблонными полями.
    let mut kinds = Vault::create(dir.join("golden_all_kinds_0123.opvault"), PASSWORD).unwrap();
    for (idx, kind) in ItemKind::all().into_iter().enumerate() {
        let mut item = Item::with_template(kind, format!("{} — образец", kind.label()));
        if matches!(kind, ItemKind::Login) {
            item.username = "user@example.com".into();
            item.password = "Kind-Pass-2026".into();
            item.url = "https://example.com".into();
        }
        for (n, field) in item.fields.iter_mut().enumerate() {
            if field.value.is_empty() {
                field.value = if field.sensitive {
                    format!("secret-{}-{}", idx, n)
                } else {
                    format!("значение-{}-{}", idx, n)
                };
            }
        }
        item.tags = vec!["образец".into()];
        item.notes = format!("заметка образца {}", idx);
        kinds.add(item).unwrap();
    }

    // Карта со сроком — для аудита и бейджей списка.
    let mut card = Item::with_template(ItemKind::Card, "Карта Тинькофф");
    card.fields = vec![
        Field::new("Владелец", "IVAN IVANOV"),
        Field::secret("Номер карты", "5536 9138 0000 1234"),
        Field::new("Срок действия", "12/27"),
        Field::secret("CVV", "123"),
        Field::new("Платёжная система", "Mastercard"),
        Field::new("Банк", "Т-Банк"),
    ];
    card.tags = vec!["финансы".into()];
    kinds.add(card).unwrap();
    kinds.save().unwrap();

    println!("готово: 3 файла в {}", dir.display());
}

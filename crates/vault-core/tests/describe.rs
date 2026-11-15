use tempfile::TempDir;
use vault_core::{Item, ItemKind, Vault};

#[test]
fn describe_missing_notes_fills_only_empty_notes() {
    let dir = TempDir::new().expect("каталог");
    let path = dir.path().join("v.opvault");
    let mut vault = Vault::create(&path, "master-password").expect("создание");

    let mut login = Item::new(ItemKind::Login, "Почта");
    login.username = "ivan".to_string();
    login.url = "https://mail.ru".to_string();
    let id_login = vault.add(login).expect("добавление");

    let mut note = Item::new(ItemKind::SecureNote, "Своя заметка");
    note.notes = "Уже есть текст.".to_string();
    let id_note = vault.add(note).expect("добавление");

    let described = vault.describe_missing_notes().expect("описания");
    assert_eq!(described, 1, "должна описаться только пустая заметка");
    let item = vault.get(id_login).expect("запись");
    assert!(
        item.notes.contains("mail.ru") && item.notes.contains("ivan"),
        "описание логина неполное: {}",
        item.notes
    );
    assert_eq!(vault.get(id_note).expect("запись").notes, "Уже есть текст.");

    // Повторный запуск не меняет ничего: заметки уже не пустые.
    assert_eq!(vault.describe_missing_notes().expect("повтор"), 0);

    // Описание попало в файл: после расшифровки оно на месте.
    let reopened = Vault::open(&path, "master-password").expect("открытие");
    assert!(
        reopened
            .get(id_login)
            .expect("запись")
            .notes
            .contains("Обновлено")
    );
}

#[test]
fn passphrase_words_and_digits_roundtrip() {
    use vault_core::{PassphraseOptions, Separator, Wordlist, generate_passphrase};

    for _ in 0..100 {
        let phrase = generate_passphrase(&PassphraseOptions::default()).expect("фраза");
        let (words, digits) = phrase.rsplit_once('-').expect("цифры в конце");
        assert_eq!(words.split('-').count(), 5, "фраза: {phrase}");
        assert_eq!(digits.len(), 2);
    }

    let en = PassphraseOptions {
        words: 3,
        digits: 0,
        separator: Separator::Space,
        capitalize: false,
        language: Wordlist::English,
    };
    let phrase = generate_passphrase(&en).expect("фраза");
    assert!(phrase.is_ascii(), "латиницей: {phrase}");
    assert_eq!(phrase.split(' ').count(), 3);
}

use std::fs;

use vault_core::{Error, GeneratorOptions, Item, ItemKind, KdfParams, Vault, generate_password};

const PASSWORD: &str = "correct horse battery";

fn temp_vault_path() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("временный каталог");
    let path = dir.path().join("test.opvault");
    (dir, path)
}

fn login(title: &str, user: &str, password: &str) -> Item {
    let mut item = Item::new(ItemKind::Login, title);
    item.username = user.into();
    item.password = password.into();
    item.url = format!("https://{}.example.com", title.to_lowercase());
    item
}

#[test]
fn roundtrip_create_add_reopen() {
    let (_dir, path) = temp_vault_path();

    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    assert!(vault.items().is_empty());
    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    vault
        .add(login("Почта", "user@example.com", "пароль"))
        .expect("add");
    drop(vault);

    let reopened = Vault::open(&path, PASSWORD).expect("открытие");
    assert_eq!(reopened.items().len(), 2);

    let github = reopened
        .items()
        .iter()
        .find(|i| i.title == "GitHub")
        .expect("запись найдена");
    assert_eq!(github.username, "octocat");
    assert_eq!(github.password, "s3cret");
}

#[test]
fn wrong_password_is_rejected() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    drop(vault);

    let err = Vault::open(&path, "wrong password here").expect_err("должна быть ошибка");
    assert!(matches!(err, Error::InvalidPasswordOrCorrupt));
}

#[test]
fn tampered_ciphertext_is_rejected() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    drop(vault);

    let mut bytes = fs::read(&path).expect("чтение");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&path, &bytes).expect("запись");

    let err = Vault::open(&path, PASSWORD).expect_err("порча должна обнаруживаться");
    assert!(matches!(err, Error::InvalidPasswordOrCorrupt));
}

#[test]
fn tampered_header_is_rejected() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    drop(vault);

    // Байты 11..15 — m_cost. Меняем на меньшее значение: заголовок входит в AAD.
    let mut bytes = fs::read(&path).expect("чтение");
    bytes[11..15].copy_from_slice(&(32 * 1024u32).to_le_bytes());
    fs::write(&path, &bytes).expect("запись");

    let result = Vault::open(&path, PASSWORD);
    assert!(
        result.is_err(),
        "подмена параметров KDF должна ломать открытие"
    );
}

#[test]
fn change_password_rotates_key() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    vault.change_password("новый мастер пароль").expect("смена");
    drop(vault);

    assert!(
        Vault::open(&path, PASSWORD).is_err(),
        "старый пароль не должен работать"
    );
    let reopened = Vault::open(&path, "новый мастер пароль").expect("новый пароль");
    assert_eq!(reopened.items().len(), 1);
}

#[test]
fn short_master_password_is_refused() {
    let (_dir, path) = temp_vault_path();
    let err = Vault::create(&path, "short").expect_err("короткий пароль недопустим");
    assert!(matches!(err, Error::InvalidFormat(_)));
}

#[test]
fn existing_file_is_not_overwritten() {
    let (_dir, path) = temp_vault_path();
    Vault::create(&path, PASSWORD).expect("создание");
    let err = Vault::create(&path, PASSWORD).expect_err("перезапись запрещена");
    assert!(matches!(err, Error::InvalidFormat(_)));
}

#[test]
fn search_matches_title_user_url_and_notes() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");

    let mut item = login("GitHub", "octocat", "s3cret");
    item.notes = "рабочий аккаунт".into();
    vault.add(item).expect("add");
    vault.add(login("Банк", "client", "pin")).expect("add");

    assert_eq!(vault.search("git").len(), 1);
    assert_eq!(vault.search("OCTOCAT").len(), 1);
    assert_eq!(vault.search("example.com").len(), 2, "url учитывается");
    assert_eq!(vault.search("рабочий").len(), 1, "заметки учитываются");
    assert_eq!(vault.search("").len(), 2, "пустой запрос — все записи");
    assert_eq!(
        vault.search("pin").len(),
        0,
        "по паролю не ищем: 'pin' есть только в пароле записи «Банк»"
    );
}

#[test]
fn password_is_not_searchable() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault
        .add(login("GitHub", "octocat", "уникальныйсекрет"))
        .expect("add");
    assert!(vault.search("уникальныйсекрет").is_empty());
}

#[test]
fn update_and_remove_work() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");

    let mut item = login("GitHub", "octocat", "s3cret");
    let id = vault.add(item.clone()).expect("add");

    item.title = "GitHub Work".into();
    item.password = "new-secret".into();
    vault.update(item).expect("update");

    assert_eq!(vault.get(id).expect("запись").title, "GitHub Work");
    assert_eq!(vault.get(id).expect("запись").password, "new-secret");

    vault.remove(id).expect("remove");
    assert!(vault.get(id).is_none());
    assert!(matches!(
        vault.remove(id).expect_err("повторное удаление"),
        Error::ItemNotFound(_)
    ));
}

#[test]
fn generator_respects_length_and_classes() {
    let opts = GeneratorOptions {
        length: 32,
        ..GeneratorOptions::default()
    };
    let password = generate_password(&opts).expect("генерация");
    assert_eq!(password.chars().count(), 32);
    assert!(password.chars().any(|c| c.is_ascii_lowercase()));
    assert!(password.chars().any(|c| c.is_ascii_uppercase()));
    assert!(password.chars().any(|c| c.is_ascii_digit()));
    assert!(password.chars().any(|c| !c.is_ascii_alphanumeric()));
    assert!(
        !password.chars().any(|c| "Il1O0o".contains(c)),
        "неоднозначные символы исключены"
    );
    assert_ne!(password, generate_password(&opts).unwrap());
}

#[test]
fn generator_rejects_empty_class_set() {
    let opts = GeneratorOptions {
        lowercase: false,
        uppercase: false,
        digits: false,
        symbols: false,
        ..GeneratorOptions::default()
    };
    assert!(generate_password(&opts).is_err());
}

#[test]
fn unicode_master_password_works() {
    let (_dir, path) = temp_vault_path();
    let password = "Мастер-пароль-с-кириллицей";
    let mut vault = Vault::create(&path, password).expect("создание");
    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    drop(vault);

    Vault::open(&path, password).expect("открытие с юникодом");
}

#[test]
fn kdf_params_are_stored_in_header() {
    let (_dir, path) = temp_vault_path();
    Vault::create(&path, PASSWORD).expect("создание");

    let bytes = fs::read(&path).expect("чтение");
    let m_cost = u32::from_le_bytes([bytes[11], bytes[12], bytes[13], bytes[14]]);
    let t_cost = u32::from_le_bytes([bytes[15], bytes[16], bytes[17], bytes[18]]);
    let default = KdfParams::default();

    assert_eq!(m_cost, default.m_cost);
    assert_eq!(t_cost, default.t_cost);
}

#[test]
fn each_save_uses_fresh_nonce() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let first = fs::read(&path).unwrap();

    vault
        .add(login("GitHub", "octocat", "s3cret"))
        .expect("add");
    let second = fs::read(&path).unwrap();

    // nonce лежит после magic+version+kdf_id+m,t,p+salt
    let nonce_start = 8 + 2 + 1 + 12 + 16;
    assert_ne!(
        &first[nonce_start..nonce_start + 24],
        &second[nonce_start..nonce_start + 24]
    );
}

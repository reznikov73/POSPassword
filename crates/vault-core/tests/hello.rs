use std::fs;
use std::path::PathBuf;

use vault_core::{Error, Item, ItemKind, KEY_LEN, Vault, generate_wrapping_key, sidecar_path};

const PASSWORD: &str = "correct horse battery";
const NEW_PASSWORD: &str = "new correct horse battery";

/// Последний байт блока wrapped_key в сайдкаре (тег GCM):
/// magic(8) + version(2) + nonce(12) + 47.
const WRAPPED_LAST_BYTE: usize = 8 + 2 + 12 + 47;

fn temp_vault_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("временный каталог");
    let path = dir.path().join("test.opvault");
    (dir, path)
}

fn login(title: &str) -> Item {
    let mut item = Item::new(ItemKind::Login, title);
    item.username = "user@example.com".into();
    item.password = "s3cret".into();
    item
}

#[test]
fn hello_roundtrip_opens_without_password() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault.add(login("GitHub")).expect("add");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    let reopened = Vault::open_with_hello(&path, &wrapping).expect("открытие через Hello");
    assert_eq!(reopened.items().len(), 1);
    assert_eq!(reopened.items()[0].title, "GitHub");
}

#[test]
fn wrong_wrapping_key_is_rejected() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    let other = generate_wrapping_key();
    let err = Vault::open_with_hello(&path, &other).expect_err("чужой ключ обёртки");
    assert!(matches!(err, Error::HelloUnwrapFailed));
}

#[test]
fn tampered_sidecar_is_rejected() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    let sidecar = sidecar_path(&path);
    let mut bytes = fs::read(&sidecar).expect("чтение сайдкара");
    bytes[WRAPPED_LAST_BYTE] ^= 0x01;
    fs::write(&sidecar, &bytes).expect("запись сайдкара");

    let err = Vault::open_with_hello(&path, &wrapping).expect_err("порча должна обнаруживаться");
    assert!(matches!(err, Error::HelloUnwrapFailed));
}

#[test]
fn sidecar_with_wrong_magic_is_rejected() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    let sidecar = sidecar_path(&path);
    let mut bytes = fs::read(&sidecar).expect("чтение сайдкара");
    bytes[0] ^= 0x01;
    fs::write(&sidecar, &bytes).expect("запись сайдкара");

    let err = Vault::open_with_hello(&path, &wrapping).expect_err("чужой формат");
    assert!(matches!(err, Error::InvalidFormat(_)));
}

#[test]
fn dpapi_blob_corruption_does_not_break_unwrap() {
    // DPAPI-блоб — забота приложения: ядро его не разбирает.
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    let sidecar = sidecar_path(&path);
    let mut bytes = fs::read(&sidecar).expect("чтение сайдкара");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&sidecar, &bytes).expect("запись сайдкара");

    Vault::open_with_hello(&path, &wrapping).expect("ключ хранилища не зависит от DPAPI-блоба");
}

#[test]
fn change_password_invalidates_hello() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault.add(login("GitHub")).expect("add");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    vault.change_password(NEW_PASSWORD).expect("смена пароля");
    drop(vault);

    assert!(!sidecar_path(&path).exists());
    let err = Vault::open_with_hello(&path, &wrapping).expect_err("сайдкара больше нет");
    assert!(matches!(err, Error::Io(_)));

    let reopened = Vault::open(&path, NEW_PASSWORD).expect("открытие с новым паролем");
    assert_eq!(reopened.items().len(), 1);
}

#[test]
fn disable_hello_is_idempotent() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    assert!(sidecar_path(&path).exists());

    vault.disable_hello().expect("отключение");
    assert!(!sidecar_path(&path).exists());
    vault
        .disable_hello()
        .expect("повторное отключение не ошибка");
    drop(vault);

    Vault::open(&path, PASSWORD).expect("хранилище открывается паролем");
}

#[test]
fn re_enabling_replaces_old_sidecar() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let old = generate_wrapping_key();
    vault.enable_hello(&old, b"blob-1").expect("enable");
    let fresh = generate_wrapping_key();
    vault
        .enable_hello(&fresh, b"blob-2")
        .expect("повторный enable");
    drop(vault);

    Vault::open_with_hello(&path, &fresh).expect("новая обёртка открывает");
    let err = Vault::open_with_hello(&path, &old).expect_err("старая обёртка больше не действует");
    assert!(matches!(err, Error::HelloUnwrapFailed));
}

#[test]
fn create_removes_stale_sidecar() {
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    // Файл хранилища удалён, сайдкар остался от прежнего ключа.
    fs::remove_file(&path).expect("удаление хранилища");
    assert!(sidecar_path(&path).exists());

    let fresh = Vault::create(&path, PASSWORD).expect("новое хранилище");
    drop(fresh);
    assert!(!sidecar_path(&path).exists());
    let err = Vault::open_with_hello(&path, &wrapping).expect_err("старая обёртка не годится");
    assert!(matches!(err, Error::Io(_)));
}

#[test]
fn vault_key_is_not_in_sidecar_plaintext() {
    // Выводим ключ так же, как ядро, и убеждаемся, что в сайдкаре его нет:
    // ключ хранилища лежит там только в шифротексте.
    let (_dir, path) = temp_vault_path();
    let mut vault = Vault::create(&path, PASSWORD).expect("создание");
    vault.add(login("GitHub")).expect("add");
    let wrapping = generate_wrapping_key();
    vault
        .enable_hello(&wrapping, b"dpapi-blob")
        .expect("enable");
    drop(vault);

    let vault_bytes = fs::read(&path).expect("чтение хранилища");
    let key = derive_key_like_core(PASSWORD, &vault_bytes);
    let sidecar_bytes = fs::read(sidecar_path(&path)).expect("чтение сайдкара");

    assert!(!sidecar_bytes.windows(KEY_LEN).any(|w| w == key));
}

/// Повторяет вывод ключа ядра по заголовку файла, чтобы проверить,
/// что сайдкар не содержит ключ в открытом виде.
fn derive_key_like_core(password: &str, bytes: &[u8]) -> [u8; KEY_LEN] {
    use argon2::{Algorithm, Argon2, Params, Version};

    let m_cost = u32::from_le_bytes(bytes[11..15].try_into().unwrap());
    let t_cost = u32::from_le_bytes(bytes[15..19].try_into().unwrap());
    let p_cost = u32::from_le_bytes(bytes[19..23].try_into().unwrap());
    let salt: [u8; 16] = bytes[23..39].try_into().unwrap();

    let params = Params::new(m_cost, t_cost, p_cost, Some(KEY_LEN)).expect("параметры из файла");
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .expect("вывод ключа");
    key
}

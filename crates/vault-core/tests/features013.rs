//! Тесты нового функционала 0.13.0: история пароля (Ф-2), резервные
//! копии (Ф-5), блокировка файла (Ф-6) и пресеты Argon2 (Б-6).

use tempfile::TempDir;
use vault_core::{Item, ItemKind, KdfPreset, PASSWORD_HISTORY_MAX, PasswordVersion, Vault};

fn dir() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("каталог");
    let path = dir.path().join("v.opvault");
    (dir, path)
}

/// История не растёт выше пяти версий, самая свежая — первой.
#[test]
fn password_history_is_capped_at_five() {
    let mut item = Item::new(ItemKind::Login, "Сайт");
    item.updated_at = 1000;
    for n in 0..8 {
        item.set_password(&format!("pass-{n}"));
        item.updated_at = 1000 + n;
    }
    assert_eq!(
        item.password_history.len(),
        PASSWORD_HISTORY_MAX,
        "история не длиннее пяти версий"
    );
    assert_eq!(item.password, "pass-7");
    // Самая свежая версия — первой.
    assert_eq!(item.password_history[0].value, "pass-6");
    assert_eq!(
        item.password_history[PASSWORD_HISTORY_MAX - 1].value,
        "pass-2"
    );
}

/// Тот же пароль не дублирует историю.
#[test]
fn same_password_does_not_pollute_history() {
    let mut item = Item::new(ItemKind::Login, "Сайт");
    item.set_password("secret-1");
    item.set_password("secret-1");
    item.set_password("secret-1");
    assert!(item.password_history.is_empty());
}

/// Восстановление версии делает её текущей, прежний уходит в историю.
#[test]
fn restoring_version_swaps_passwords() {
    let mut item = Item::new(ItemKind::Login, "Сайт");
    item.set_password("old-1");
    item.updated_at = 100;
    item.set_password("old-2");
    item.updated_at = 200;
    item.set_password("current");
    assert_eq!(item.password_history.len(), 2);

    item.restore_password_version(1).expect("восстановление");
    assert_eq!(item.password, "old-1", "восстановлена вторая версия");
    assert_eq!(
        item.password_history[0].value, "current",
        "прежний текущий уходит в историю первым"
    );
    assert_eq!(item.password_history[0].changed_at, 200);
    assert!(
        item.restore_password_version(9).is_err(),
        "нет такого индекса"
    );
}

/// История переживает сохранение в файл и обратно.
#[test]
fn password_history_roundtrips_through_file() {
    let (_dir, path) = dir();
    let mut vault = Vault::create(&path, "master-password").expect("создание");
    let mut item = Item::new(ItemKind::Login, "Сайт");
    item.set_password("первый");
    item.updated_at = 1000;
    item.set_password("второй");
    item.updated_at = 2000;
    item.favorite = true;
    let id = vault.add(item).expect("добавление");

    // Смена пароля через update: прежний текущий уходит в историю.
    let mut edited = vault.get(id).expect("запись").clone();
    edited.set_password("третий");
    vault.update(edited).expect("обновление");
    drop(vault);

    let reopened = Vault::open(&path, "master-password").expect("открытие");
    let item = reopened.get(id).expect("запись");
    assert_eq!(item.password, "третий");
    assert_eq!(item.password_history.len(), 2);
    assert_eq!(item.password_history[0].value, "второй");
    assert_eq!(item.password_history[1].value, "первый");
    assert!(item.favorite, "флаг избранного пережил файл");
}

/// Смена пароля через Vault::update работает без set_password:
/// так пишет историю мост браузера.
#[test]
fn update_writes_history_for_any_caller() {
    let (_dir, path) = dir();
    let mut vault = Vault::create(&path, "master-password").expect("создание");
    let mut item = Item::new(ItemKind::Login, "Сайт");
    item.password = "из-браузера-старый".into();
    let id = vault.add(item).expect("добавление");

    let mut edited = vault.get(id).expect("запись").clone();
    edited.password = "из-браузера-новый".into();
    vault.update(edited).expect("обновление");
    assert_eq!(
        vault.get(id).expect("запись").password_history.len(),
        1,
        "мост браузера тоже пишет историю"
    );
}

/// .bak создаётся при повторном сохранении и содержит прежнюю версию.
#[test]
fn backup_file_keeps_previous_version() {
    let (_dir, path) = dir();
    let mut vault = Vault::create(&path, "master-password").expect("создание");
    let first = Item::new(ItemKind::Login, "Первая");
    vault.add(first).expect("добавление");
    let raw_first = std::fs::read(&path).expect("файл");

    let second = Item::new(ItemKind::Login, "Вторая");
    vault.add(second).expect("добавление");
    drop(vault);

    let backup = vault_core::backup_path(&path);
    assert!(backup.exists(), "после второго сохранения есть .bak");
    assert_eq!(
        std::fs::read(&backup).expect("bak"),
        raw_first,
        "в .bak — побайтово прежняя версия"
    );
    // Резервная копия открывается как обычное хранилище.
    let restored = Vault::open(&backup, "master-password").expect("bak открывается");
    assert_eq!(restored.items().len(), 1);
    assert_eq!(restored.items()[0].title, "Первая");
}

/// Открытое хранилище блокирует второй процесс (модель KeePass, Ф-6).
#[test]
fn open_vault_locks_out_second_opener() {
    let (_dir, path) = dir();
    let vault = Vault::create(&path, "master-password").expect("создание");
    let err = Vault::open(&path, "master-password").expect_err("второе открытие не должно пройти");
    assert!(
        err.to_string().contains("занято"),
        "ошибка должна быть внятной: {err}"
    );
    drop(vault);
    // После освобождения — открывается.
    Vault::open(&path, "master-password").expect("после закрытия открывается");
}

/// Пресеты Argon2 пишутся в заголовок и читаются обратно.
#[test]
fn kdf_presets_persist_in_header() {
    let (_dir, path) = dir();
    let vault = Vault::create_with_preset(&path, "master-password", KdfPreset::Hardened)
        .expect("создание с пресетом");
    assert_eq!(vault.kdf_params().m_cost, 128 * 1024);
    drop(vault);

    let reopened = Vault::open(&path, "master-password").expect("открытие с теми же параметрами");
    assert_eq!(reopened.kdf_params().m_cost, 128 * 1024);
    drop(reopened);

    // Апгрейд пресета при смене мастер-пароля.
    let mut again = Vault::open(&path, "master-password").expect("открытие");
    again
        .change_password_with_preset("master-password-2", KdfPreset::Maximum.params())
        .expect("смена с пресетом");
    drop(again);
    let upgraded = Vault::open(&path, "master-password-2").expect("новый пароль");
    assert_eq!(upgraded.kdf_params().m_cost, 256 * 1024);
}

/// Версии пароля с одинаковым значением не заносятся в историю повторно
/// (после восстановления текущего).
#[test]
fn history_after_restore_does_not_duplicate() {
    let mut item = Item::new(ItemKind::Login, "Сайт");
    item.set_password("v1");
    item.set_password("v2");
    item.restore_password_version(0).expect("восстановить v1");
    // v1 уже текущий; повторная запись v1 ничего не меняет.
    assert_eq!(item.password, "v1");
    assert_eq!(item.password_history.len(), 1);
    assert_eq!(item.password_history[0].value, "v2");
}

/// Drop версии пароля зануляет значение: сам факт зануления — контракт
/// крейта zeroize и реализации Drop в model.rs; здесь проверяем, что
/// структура истории корректно переживает удаление элементов.
#[test]
fn history_truncation_drops_old_versions_safely() {
    let mut item = Item::new(ItemKind::Login, "Сайт");
    for n in 0..10 {
        item.set_password(&format!("p{n}"));
    }
    assert_eq!(item.password_history.len(), PASSWORD_HISTORY_MAX);
    assert_eq!(item.password_history[0].value, "p8");
    // PasswordVersion используется только внутри модели и истории.
    let _version = PasswordVersion {
        value: String::new(),
        changed_at: 0,
    };
}

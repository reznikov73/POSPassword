//! Блокировка файла хранилища от параллельной записи (Ф-6 из ТЗ 0.13.0).
//!
//! Модель KeePass: сайдкар `<файл>.lock` с ОС-блокировкой (flock в Linux,
//! LockFileEx в Windows). Приложение держит лок, пока хранилище открыто,
//! CLI — на всё время команды. Это закрывает и повреждение (его и так
//! гарантирует атомарное переименование), и потерю правок вторым писателем.
//!
//! Упавший процесс освобождает лок сам: блокировка живёт, пока открыт
//! дескриптор файла. Сам файл-сайдкар после этого остаётся пустым — это
//! нормально и ни на что не влияет.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use crate::{Error, Result};

/// Путь сайдкар-лока: `<файл-хранилища>.lock`.
pub fn lock_path(vault_path: &Path) -> PathBuf {
    let mut name = vault_path.as_os_str().to_os_string();
    name.push(".lock");
    PathBuf::from(name)
}

/// Захваченный лок. Освобождается при уничтожении структуры.
#[derive(Debug)]
pub struct FileLock {
    /// Дескриптор держит блокировку; закрытие (в том числе при крахе
    /// процесса) освобождает её.
    _file: File,
}

impl FileLock {
    /// Удерживается ли блокировка (файл открыт).
    pub fn holds(&self) -> bool {
        true
    }

    /// Захватывает исключительную блокировку. Если хранилище уже открыто
    /// в другом процессе — внятная ошибка «занято».
    pub fn acquire(vault_path: &Path) -> Result<Self> {
        let path = lock_path(vault_path);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| {
                Error::Busy(format!(
                    "не открыть файл блокировки {}: {err}",
                    path.display()
                ))
            })?;
        // try_lock_exclusive возвращает Ok(false), если лок занят: это
        // не ошибка системного вызова, а именно «другой процесс держит».
        match file.try_lock_exclusive() {
            Ok(true) => Ok(Self { _file: file }),
            Ok(false) => Err(Error::Busy(format!(
                "хранилище открыто в другом процессе ({})",
                path.display()
            ))),
            Err(err) => Err(Error::Busy(format!(
                "не удалось заблокировать {}: {err}",
                path.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Проверка в два процесса (как GUI и CLI): дочерний запуск этого же
    /// тестового бинарника с переменной окружения пробует взять занятый лок.
    #[test]
    fn second_process_lock_is_rejected() {
        if let Ok(child_dir) = std::env::var("HRANITEL_LOCK_CHILD") {
            let path = std::path::Path::new(&child_dir).join("vault.opvault");
            std::process::exit(if FileLock::acquire(&path).is_ok() {
                0
            } else {
                1
            });
        }

        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault.opvault");
        let first = FileLock::acquire(&vault).expect("первый лок берётся");

        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("lock::tests::second_process_lock_is_rejected")
            .env("HRANITEL_LOCK_CHILD", dir.path())
            .status()
            .expect("запустить дочерний процесс теста");
        assert_ne!(
            status.code(),
            Some(0),
            "второй процесс не должен взять занятый лок"
        );

        drop(first);
        assert!(
            FileLock::acquire(&vault).is_ok(),
            "после освобождения лок берётся"
        );
    }
}

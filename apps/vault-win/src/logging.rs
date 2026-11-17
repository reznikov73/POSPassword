//! Лог-файл с ротацией (Т-4 из ТЗ 0.13.0).
//!
//! Пишется `%APPDATA%\Hranitel\log.txt`; при превышении лимита файл
//! сдвигается: `log.txt` → `log.1` → `log.2` (третья копия удаляется).
//! В лог попадают только уровни, действия, id записей, PID и имена
//! образов — названия записей и любые секреты не пишутся никогда.

use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Предел одного файла лога.
const LIMIT: u64 = 1024 * 1024;
/// Сколько поколений хранить: log.txt, log.1, log.2.
const GENERATIONS: usize = 2;

/// Настраивает подписчика: события трассировки идут в файл.
///
/// Вызывается один раз при запуске. Если каталог недоступен, программа
/// продолжает работать без лога — пользователь не должен терять доступ
/// к паролям из-за журнала.
pub fn init() {
    let Some(path) = log_path() else {
        return;
    };
    let shared = SharedLog::open(path);
    let builder = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(shared);
    let _ = builder.try_init();
}

fn log_path() -> Option<PathBuf> {
    let dir = crate::settings::app_dir();
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("log.txt"))
}

/// Общий файл лога между потоками подписчика.
#[derive(Clone)]
struct SharedLog {
    inner: Arc<Mutex<LogFile>>,
}

struct LogFile {
    path: PathBuf,
    file: File,
    written: u64,
}

impl SharedLog {
    fn open(path: PathBuf) -> Self {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        match file {
            Some(file) => {
                let written = file.metadata().map(|m| m.len()).unwrap_or(0);
                Self {
                    inner: Arc::new(Mutex::new(LogFile {
                        path,
                        file,
                        written,
                    })),
                }
            }
            // Файл недоступен — пустой лог-приёмник, чтобы программа работала.
            None => Self {
                inner: Arc::new(Mutex::new(LogFile {
                    path: PathBuf::new(),
                    file: dev_null(),
                    written: 0,
                })),
            },
        }
    }
}

/// Файл-заглушка, выбрасывающая записи.
fn dev_null() -> File {
    #[cfg(windows)]
    let path = "NUL";
    #[cfg(not(windows))]
    let path = "/dev/null";
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|_| {
            // Совсем без файла: stdin как несерьёзный запасной вариант.
            File::create(PathBuf::from(path)).unwrap_or_else(|_| panic!("нет ни лога, ни заглушки"))
        })
}

impl LogFile {
    fn rotate(&mut self) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        // Сдвиг поколений: log.1 → log.2, log.txt → log.1.
        for generation in (1..=GENERATIONS).rev() {
            let from = self.path.with_extension(generation.to_string());
            let to = self.path.with_extension((generation + 1).to_string());
            if generation == GENERATIONS {
                let _ = std::fs::remove_file(&from);
            } else {
                let _ = std::fs::rename(&from, to);
            }
        }
        let _ = std::fs::rename(&self.path, self.path.with_extension("1"));
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            self.file = file;
            self.written = 0;
        }
    }
}

/// Одна строка лога: захватывает мьютекс при записи.
struct LogLine(Arc<Mutex<LogFile>>);

impl std::io::Write for LogLine {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut log = self.0.lock().expect("лог не отравлен");
        let written = log.file.write(buf)?;
        log.written += written as u64;
        if log.written > LIMIT {
            log.rotate();
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut log = self.0.lock().expect("лог не отравлен");
        log.file.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedLog {
    type Writer = LogLine;

    fn make_writer(&'a self) -> Self::Writer {
        LogLine(Arc::clone(&self.inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn rotates_when_over_limit() {
        let dir = tempfile::tempdir().expect("каталог");
        let path = dir.path().join("log.txt");
        // Записываем больше лимита мелкими кусками.
        for _ in 0..(LIMIT / 1024 + 2) {
            let mut line = LogLine(SharedLog::open(path.clone()).inner);
            line.write_all(&[b'x'; 1024]).expect("запись");
        }
        assert!(path.with_extension("1").exists(), "лог сдвинулся в log.1");
        assert!(path.exists(), "текущий лог на месте");
    }
}

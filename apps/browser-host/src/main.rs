//! Родной хост для браузерного расширения «Хранителя» (Native Messaging).
//!
//! Браузер запускает эту программу сам и общается с ней по stdin/stdout
//! сообщениями в рамке «4 байта длины + JSON». Хост ничего не решает и
//! ничего не хранит: он пересылает запрос в «Хранитель» через именованный
//! канал `\\.\pipe\hranitel-bridge` и возвращает ответ браузеру. Если
//! «Хранитель» не запущен или хранилище заблокировано, ответ об этом
//! приходит от приложения, а не выдумывается здесь.
//!
//! Никаких сетевых соединений: только stdin/stdout и локальный канал.

// Хост запускает браузер — консольное окно рядом не нужно. В отладочной
// сборке консоль остаётся, чтобы смотреть логи (они пишутся в stderr).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod codec;

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    {
        win::run();
    }
    #[cfg(not(windows))]
    {
        eprintln!("browser-host работает только в Windows");
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(windows)]
mod win {
    use std::io::{Read, Write};
    use std::time::Duration;

    use windows::core::w;
    use windows::Win32::Foundation::{ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING,
    };

    use super::codec;

    /// Читает запросы браузера со stdin и отвечает на stdout.
    pub fn run() {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        loop {
            let request = match codec::read_frame(&mut stdin.lock()) {
                Ok(Some(frame)) => frame,
                // Браузер закрыл порт — сеанс окончен.
                Ok(None) => break,
                Err(err) => {
                    eprintln!("browser-host: не удалось прочитать запрос: {err}");
                    break;
                }
            };
            let reply = exchange_with_app(&request);
            if codec::write_frame(&mut stdout.lock(), &reply).is_err() {
                break;
            }
        }
    }

    /// Один обмен с приложением: подключиться, отправить запрос, забрать ответ.
    /// Если канала нет, пробуем поднять «Хранитель» — клик по замку в браузере
    /// должен сам открыть программу с запросом на разблокировку.
    fn exchange_with_app(request: &[u8]) -> Vec<u8> {
        let mut pipe = match PipeClient::connect() {
            Ok(pipe) => pipe,
            Err(first) => match start_app_and_connect() {
                Ok(pipe) => pipe,
                Err(err) => {
                    return error_reply(&format!(
                        "«Хранитель» не отвечает ({first}; {err}). Запустите программу и разблокируйте хранилище."
                    ));
                }
            },
        };
        if let Err(err) = codec::write_frame(&mut pipe, request) {
            return error_reply(&format!("не удалось отправить запрос: {err}"));
        }
        match codec::read_frame(&mut pipe) {
            Ok(Some(reply)) => reply,
            Ok(None) => error_reply("«Хранитель» закрыл соединение."),
            Err(err) => error_reply(&format!("не удалось прочитать ответ: {err}")),
        }
    }

    fn error_reply(message: &str) -> Vec<u8> {
        let json = serde_json::json!({ "action": "error", "message": message });
        serde_json::to_vec(&json).unwrap_or_default()
    }

    /// Запускает `vault-win.exe` рядом с хостом (свёрнутым в трей) и ждёт
    /// канал до трёх секунд. `--quiet` — чтобы второй экземпляр не показывал
    /// окно «уже запущен».
    fn start_app_and_connect() -> Result<PipeClient, String> {
        let host = std::env::current_exe().map_err(|err| err.to_string())?;
        let dir = host
            .parent()
            .ok_or_else(|| "не удалось определить папку хоста".to_string())?;
        let app = dir.join("vault-win.exe");
        if !app.exists() {
            return Err("vault-win.exe рядом с browser-host.exe не найден".to_string());
        }
        std::process::Command::new(&app)
            .arg("--tray")
            .arg("--quiet")
            // Не наследовать stdin/stdout хоста: иначе браузер будет ждать
            // закрытия порта, пока живёт запущенное приложение.
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|err| format!("не удалось запустить «Хранитель»: {err}"))?;

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            std::thread::sleep(Duration::from_millis(150));
            if let Ok(pipe) = PipeClient::connect() {
                return Ok(pipe);
            }
            if std::time::Instant::now() >= deadline {
                return Err("«Хранитель» не успел открыть канал".to_string());
            }
        }
    }

    /// Клиент именованного канала с интерфейсом `Read`/`Write` — чтобы
    /// переиспользовать общий код рамки.
    struct PipeClient(windows::Win32::Foundation::HANDLE);

    impl PipeClient {
        /// Канал появляется вместе с «Хранителем»: если браузер стартовал
        /// раньше, немного ждём и пробуем снова.
        fn connect() -> Result<Self, String> {
            let mut waited_ms = 0u32;
            loop {
                // SAFETY: имя канала — статическая строка, атрибуты не нужны.
                let result = unsafe {
                    CreateFileW(
                        w!(r"\\.\pipe\hranitel-bridge"),
                        (GENERIC_READ | GENERIC_WRITE).0,
                        FILE_SHARE_MODE(0),
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    )
                };
                match result {
                    Ok(handle) => return Ok(Self(handle)),
                    Err(err) => {
                        let busy = err.code() == ERROR_PIPE_BUSY.to_hresult();
                        let (pause, limit) = if busy {
                            (50u32, 2000u32)
                        } else {
                            (200u32, 2000u32)
                        };
                        if waited_ms < limit {
                            std::thread::sleep(Duration::from_millis(pause as u64));
                            waited_ms += pause;
                            continue;
                        }
                        return Err("приложение не запущено".to_string());
                    }
                }
            }
        }
    }

    impl Read for PipeClient {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut read = 0u32;
            // SAFETY: буфер валиден на всю длину вызова; канал блокирующий.
            let result = unsafe {
                windows::Win32::Storage::FileSystem::ReadFile(
                    self.0,
                    Some(buf),
                    Some(&mut read),
                    None,
                )
            };
            match result {
                Ok(()) => Ok(read as usize),
                Err(err)
                    if err.code() == windows::Win32::Foundation::ERROR_BROKEN_PIPE.to_hresult() =>
                {
                    Ok(0)
                }
                Err(err) => Err(std::io::Error::other(err.to_string())),
            }
        }
    }

    impl Write for PipeClient {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut written = 0u32;
            // SAFETY: буфер валиден на всю длину вызова; канал блокирующий.
            let result = unsafe {
                windows::Win32::Storage::FileSystem::WriteFile(
                    self.0,
                    Some(buf),
                    Some(&mut written),
                    None,
                )
            };
            match result {
                Ok(()) => Ok(written as usize),
                Err(err) => Err(std::io::Error::other(err.to_string())),
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            // SAFETY: сброс буферов дескриптора канала.
            unsafe { windows::Win32::Storage::FileSystem::FlushFileBuffers(self.0) }
                .map_err(|err| std::io::Error::other(err.to_string()))
        }
    }
}

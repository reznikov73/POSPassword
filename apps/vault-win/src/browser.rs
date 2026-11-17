//! Сервер моста к браузерному расширению (модель KeePassXC).
//!
//! Расширение браузера разговаривает с родным хостом (крейт `browser-host`)
//! через Native Messaging, а хост подключается к «Хранителю» по именованному
//! каналу `\\.\pipe\hranitel-bridge`. Обмен — те же рамки «4 байта длины +
//! JSON». Приложение — единственный владелец хранилища: расширение не умеет
//! читать файл и получает только то, что ему разрешили.
//!
//! Порядок работы: поток канала принимает запрос, передаёт его в UI через
//! канал (`BrowserJob`) и ждёт ответа. UI решает сам: простые запросы (пинг,
//! генератор) закрываются сразу, запросы доступа и сохранения сначала
//! показывают диалог и отвечают после решения пользователя.

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{PCWSTR, PWSTR, w};

/// Сколько поток ждёт решения пользователя в диалоге, прежде чем ответеть
/// отказом. Хватает на неторопливое чтение вопроса.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

/// Максимальный размер сообщения — как в кодеке родного хоста.
const MAX_MESSAGE: usize = 1024 * 1024;

/// Запрос расширения. Поля — JSON в kebab-case, как в протоколе.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum BrowserRequest {
    Ping,
    GetLogins {
        url: String,
    },
    GetCredentials {
        id: Uuid,
        #[serde(default)]
        url: String,
    },
    SaveLogin {
        url: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        #[serde(default)]
        title: String,
    },
    GeneratePassword,
}

/// Одно задание потоку UI: запрос и труба для ответа.
pub struct BrowserJob {
    pub request: BrowserRequest,
    pub reply: Sender<String>,
}

/// Запускает поток-сервер именованного канала. Возвращает канал заданий,
/// который UI опрашивает каждый кадр. Поток живёт до конца программы —
/// как и горячие клавиши, канал не должен зависеть от кадров интерфейса.
pub fn start(ctx: eframe::egui::Context) -> Receiver<BrowserJob> {
    let (jobs, inbox) = channel();
    std::thread::spawn(move || {
        // DACL канала: полный доступ только текущему пользователю и SYSTEM
        // (Б-3а). Строится один раз на процесс; при неудаче — атрибуты по
        // умолчанию (канал всё равно требует диалога и разрешения домена).
        let security = pipe_security();
        loop {
            // SAFETY: имя канала — статическая строка; дескриптор безопасности
            // живёт, пока живёт структура `security`. При ошибке возвращается
            // невалидный дескриптор, а не Result.
            let pipe = unsafe {
                CreateNamedPipeW(
                    w!(r"\\.\pipe\hranitel-bridge"),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    64 * 1024,
                    64 * 1024,
                    0,
                    security.as_ref().map(|sec| &sec.attributes as *const _),
                )
            };
            if pipe.is_invalid() {
                // Канал занят старым экземпляром: подождём и попробуем снова.
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }

            // Ждём подключения родного хоста (блокирующий вызов).
            if unsafe { ConnectNamedPipe(pipe, None) }.is_err() {
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(pipe) };
                continue;
            }
            // PID и образ подключившегося — в лог для разбора инцидентов.
            log_client(pipe);
            serve_client(pipe, &jobs, &ctx);
            let _ = unsafe { FlushFileBuffers(pipe) };
            let _ = unsafe { DisconnectNamedPipe(pipe) };
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(pipe) };
        }
    });
    inbox
}

/// Атрибуты безопасности канала: дескриптор и структура, которая на него
/// указывает. Дескриптор освобождается при уничтожении структуры.
struct PipeSecurity {
    attributes: SECURITY_ATTRIBUTES,
    _descriptor: PSECURITY_DESCRIPTOR,
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        if !self._descriptor.is_invalid() {
            // SAFETY: дескриптор создан ConvertStringSecurityDescriptorToSecurityDescriptorW
            // и больше не используется.
            unsafe { LocalFree(Some(HLOCAL(self._descriptor.0 as *mut _))) };
        }
    }
}

/// DACL именованного канала: полный доступ только текущему пользователю
/// и SYSTEM (Б-3а). Пока канал не проверяет, кто подключился, чужая
/// учётная запись этой машины раньше могла достучаться до диалога.
fn pipe_security() -> Option<PipeSecurity> {
    // SAFETY: все вызовы Win32; буферы живут в этой функции.
    unsafe {
        // SID текущего пользователя из маркера доступа процесса.
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buffer = vec![0u8; len as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut _),
            len,
            &mut len,
        )
        .ok()?;
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut sid_text = PWSTR::null();
        ConvertSidToStringSidW(PSID(user.User.Sid.0), &mut sid_text).ok()?;
        let sid = sid_text.to_string().ok()?;
        let _ = LocalFree(Some(HLOCAL(sid_text.as_ptr() as *mut _)));
        let _ = windows::Win32::Foundation::CloseHandle(token);

        // SDDL: доступ SYSTEM и этому пользователю, больше никому.
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .ok()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: windows::core::BOOL::default(),
        };
        Some(PipeSecurity {
            attributes,
            _descriptor: descriptor,
        })
    }
}

/// Пишет PID и имя образа подключившегося клиента в лог (Б-3а).
///
/// Это диагностика для разбора инцидентов, а не граница защиты: имя образа
/// подделывается, а легальный путь установки произволен. Настоящие границы —
/// DACL канала, диалоги разрешений и (в 0.14) общий секрет с расширением.
fn log_client(pipe: HANDLE) {
    // SAFETY: дескриптор канала валиден; буферы локальны.
    unsafe {
        let mut pid = 0u32;
        if GetNamedPipeClientProcessId(pipe, &mut pid).is_err() {
            return;
        }
        let image = if pid == 0 {
            String::new()
        } else {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .ok()
                .and_then(|process| {
                    let mut buffer = [0u16; 1024];
                    let mut length = buffer.len() as u32;
                    QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_WIN32,
                        PWSTR(buffer.as_mut_ptr()),
                        &mut length,
                    )
                    .ok()
                    .map(|_| String::from_utf16_lossy(&buffer[..length as usize]))
                })
                .unwrap_or_else(|| format!("pid {pid}, образ неизвестен"))
        };
        // Б-3а: диагностика подключений. Названия записей и секреты в лог
        // не пишутся — только PID, образ и факт.
        tracing::info!(pid, image = %image, "мост: подключение клиента");
        if !image.contains("browser-host") && !image.is_empty() {
            tracing::warn!(image = %image, "мост: подключился не browser-host");
        }
    }
}

/// Обслуживает одно подключение: запрос — ответ, пока хост не отключится.
fn serve_client(pipe: HANDLE, jobs: &Sender<BrowserJob>, ctx: &eframe::egui::Context) {
    loop {
        let frame = match read_frame(pipe) {
            Ok(Some(frame)) => frame,
            // Хост закрыл соединение — обычный конец сеанса.
            Ok(None) => return,
            Err(_) => return,
        };

        let request = match serde_json::from_slice::<BrowserRequest>(&frame) {
            Ok(request) => request,
            Err(err) => {
                let reply = error_json(&format!("неизвестный запрос: {err}"));
                if write_frame(pipe, &reply).is_err() {
                    return;
                }
                continue;
            }
        };

        // Пинг и генератор хранилища не требуют и кадров интерфейса не ждут:
        // отвечаем прямо здесь, иначе спрятанное в трей окно «молчит».
        match &request {
            BrowserRequest::Ping => {
                if write_frame(pipe, &pong_json()).is_err() {
                    return;
                }
                continue;
            }
            BrowserRequest::GeneratePassword => {
                let reply = match vault_core::generate_passphrase(
                    &vault_core::PassphraseOptions::default(),
                ) {
                    Ok(value) => password_json(&value),
                    Err(err) => error_json(&format!("генератор: {err}")),
                };
                if write_frame(pipe, &reply).is_err() {
                    return;
                }
                continue;
            }
            _ => {}
        }

        // Запросы доступа и сохранения ждут человека: если окно свёрнуто,
        // ответа не будет — значит, окно надо поднять, иначе пользователь
        // вообще не узнает о вопросе.
        let attention = needs_attention(&request);
        if attention {
            // Спрятанное окно не получает кадров — показываем заранее.
            crate::autotype::restore_main_window();
        }
        let (reply, answer) = channel();
        if jobs.send(BrowserJob { request, reply }).is_err() {
            let _ = write_frame(pipe, &error_json("«Хранитель» закрывается."));
            return;
        }
        ctx.request_repaint();
        let answered = wait_answer(&answer, attention, ctx);
        if write_frame(pipe, &answered).is_err() {
            return;
        }
    }
}

/// Требует ли запрос решения человека.
fn needs_attention(request: &BrowserRequest) -> bool {
    !matches!(
        request,
        BrowserRequest::Ping | BrowserRequest::GeneratePassword
    )
}

/// Ждёт ответа UI; при необходимости поднимает окно и будит интерфейс.
fn wait_answer(answer: &Receiver<String>, attention: bool, ctx: &eframe::egui::Context) -> String {
    let deadline = Instant::now() + REPLY_TIMEOUT;
    let mut woken = false;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return error_json("пользователь не ответил за 2 минуты");
        }
        let wait = (deadline - now).min(Duration::from_millis(1500));
        match answer.recv_timeout(wait) {
            Ok(reply) => return reply,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if attention && !woken {
                    crate::autotype::restore_main_window();
                    ctx.request_repaint();
                    woken = true;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return error_json("интерфейс «Хранителя» закрылся");
            }
        }
    }
}

/// Файл со списком доменов, к которым расширение имеет постоянный доступ.
pub fn domains_file(dir: &Path) -> std::path::PathBuf {
    dir.join("browser-sites.txt")
}

/// Читает список разрешённых доменов.
pub fn load_domains(dir: &Path) -> HashSet<String> {
    std::fs::read_to_string(domains_file(dir))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Дописывает домен в список разрешённых.
pub fn remember_domain(dir: &Path, domain: &str) {
    let path = domains_file(dir);
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    text.push_str(domain);
    text.push('\n');
    let _ = std::fs::write(&path, text);
}

/// Читает одну рамку из канала. `Ok(None)` — клиент отключился.
fn read_frame(pipe: HANDLE) -> Result<Option<Vec<u8>>, String> {
    let mut head = [0u8; 4];
    if !read_exact(pipe, &mut head)? {
        return Ok(None);
    }
    let length = u32::from_le_bytes(head) as usize;
    if length == 0 {
        return Ok(Some(Vec::new()));
    }
    if length > MAX_MESSAGE {
        return Err("сообщение длиннее мегабайта".to_string());
    }
    let mut frame = vec![0u8; length];
    if !read_exact(pipe, &mut frame)? {
        return Ok(None);
    }
    Ok(Some(frame))
}

/// Дочитывает буфер до конца: `ReadFile` возвращает и частичные порции.
/// `false` — поток закрылся до начала чтения.
fn read_exact(pipe: HANDLE, buffer: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < buffer.len() {
        let mut read = 0u32;
        // SAFETY: буфер валиден на всю длину вызова; канал блокирующий.
        let result = unsafe { ReadFile(pipe, Some(&mut buffer[filled..]), Some(&mut read), None) };
        match result {
            Ok(()) if read == 0 => return Ok(filled == 0),
            Ok(()) => filled += read as usize,
            Err(err)
                if err.code() == windows::Win32::Foundation::ERROR_BROKEN_PIPE.to_hresult() =>
            {
                return Ok(filled == 0);
            }
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(true)
}

/// Пишет одну рамку в канал.
fn write_frame(pipe: HANDLE, data: &str) -> Result<(), String> {
    let bytes = (data.len() as u32).to_le_bytes();
    write_all(pipe, &bytes)?;
    write_all(pipe, data.as_bytes())
}

fn write_all(pipe: HANDLE, mut data: &[u8]) -> Result<(), String> {
    while !data.is_empty() {
        let mut written = 0u32;
        // SAFETY: буфер валиден на всю длину вызова; канал блокирующий.
        let result = unsafe { WriteFile(pipe, Some(data), Some(&mut written), None) };
        match result {
            Ok(()) if written == 0 => return Err("канал закрыт".to_string()),
            Ok(()) => data = &data[written as usize..],
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(())
}

/// JSON-ответ об ошибке.
pub fn error_json(message: &str) -> String {
    serde_json::to_string(&serde_json::json!({ "action": "error", "message": message }))
        .unwrap_or_default()
}

/// Список записей для сайта: без паролей — только заголовки и логины.
pub fn logins_json(url: &str, items: &[(Uuid, String, String, String)]) -> String {
    let list: Vec<Value> = items
        .iter()
        .map(|(id, title, username, item_url)| {
            serde_json::json!({
                "id": id.to_string(),
                "title": title,
                "username": username,
                "url": item_url,
            })
        })
        .collect();
    serde_json::to_string(&serde_json::json!({ "action": "logins", "url": url, "items": list }))
        .unwrap_or_default()
}

/// Ответ с логином и паролем конкретной записи.
pub fn credentials_json(username: &str, password: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "action": "credentials",
        "username": username,
        "password": password,
    }))
    .unwrap_or_default()
}

/// Ответ на запрос сохранения.
pub fn saved_json(created: bool, title: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "action": "saved",
        "created": created,
        "title": title,
    }))
    .unwrap_or_default()
}

/// Свежий пароль по настройкам генератора по умолчанию.
pub fn password_json(value: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "action": "password",
        "value": value,
    }))
    .unwrap_or_default()
}

/// Понг с номером версии приложения.
pub fn pong_json() -> String {
    serde_json::to_string(&serde_json::json!({
        "action": "pong",
        "version": env!("CARGO_PKG_VERSION"),
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_parse_from_kebab_case_json() {
        let ping: BrowserRequest = serde_json::from_str(r#"{"action":"ping"}"#).expect("пинг");
        assert!(matches!(ping, BrowserRequest::Ping));

        let logins: BrowserRequest =
            serde_json::from_str(r#"{"action":"get-logins","url":"https://lk.domclick.ru"}"#)
                .expect("список");
        match logins {
            BrowserRequest::GetLogins { url } => assert_eq!(url, "https://lk.domclick.ru"),
            other => panic!("не тот запрос: {other:?}"),
        }

        let save: BrowserRequest = serde_json::from_str(
            r#"{"action":"save-login","url":"https://x.ru","username":"i","password":"p","title":"X"}"#,
        )
        .expect("сохранение");
        match save {
            BrowserRequest::SaveLogin {
                url,
                username,
                password,
                title,
            } => {
                assert_eq!((url.as_str(), username.as_str()), ("https://x.ru", "i"));
                assert_eq!((password.as_str(), title.as_str()), ("p", "X"));
            }
            other => panic!("не тот запрос: {other:?}"),
        }
    }

    #[test]
    fn unknown_action_is_rejected() {
        assert!(serde_json::from_str::<BrowserRequest>(r#"{"action":"nuke"}"#).is_err());
    }

    #[test]
    fn json_replies_are_shaped_for_the_extension() {
        assert!(pong_json().contains(r#""action":"pong""#));
        let logins = logins_json(
            "https://x.ru",
            &[(Uuid::new_v4(), "Сайт".into(), "ivan".into(), "x.ru".into())],
        );
        assert!(logins.contains(r#""action":"logins""#));
        assert!(logins.contains("ivan"));
        assert!(!logins.contains("password"));
    }
}

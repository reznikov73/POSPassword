//! Автоввод логина и пароля в активное окно (аналог Auto-Type в KeePass).
//!
//! Глобальная горячая клавиша Ctrl+Alt+A регистрируется на отдельном потоке:
//! WM_HOTKEY приходит в его очередь сообщений, поток сам печатает данные через
//! `SendInput` (KEYEVENTF_UNICODE — работают русские пароли и любая раскладка)
//! и просит egui перерисоваться, чтобы показать результат в строке состояния.
//!
//! Почему не в UI-кадре: когда окно свёрнуто, eframe не вызывает `ui()`,
//! а автоввод должен работать именно из свёрнутого приложения.

use std::sync::{Mutex, OnceLock};
use std::thread;

use eframe::egui;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, SendInput, VIRTUAL_KEY,
    VK_RETURN, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG, PM_NOREMOVE, PeekMessageW,
    WM_HOTKEY,
};
use zeroize::Zeroizing;

/// Идентификатор горячей клавиши — произвольный, лишь бы уникальный в программе.
const HOTKEY_ID: i32 = 0x4A1;
/// Виртуальный код клавиши A.
const KEY_A: u32 = 0x41;

/// Что печатать по горячей клавише: заголовок для отчёта и сами данные.
#[derive(Clone)]
pub struct Credentials {
    pub title: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

struct Shared {
    /// Текущая выбранная запись. `None` — хранилище заблокировано или ничего не выбрано.
    credentials: Mutex<Option<Credentials>>,
    /// Что показать в строке состояния после последнего нажатия.
    message: Mutex<Option<String>>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        credentials: Mutex::new(None),
        message: Mutex::new(None),
    })
}

/// Регистрирует Ctrl+Alt+A и запускает поток ожидания нажатия.
///
/// Ошибка означает, что комбинацию заняла другая программа; автоввод в этом
/// случае недоступен, остальное приложение работает как обычно.
pub fn start(ctx: egui::Context) -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::channel();

    thread::spawn(move || {
        // Очередь сообщений потока создаётся при первом обращении к ней.
        let mut message = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        }

        let registered =
            unsafe { RegisterHotKey(None, HOTKEY_ID, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, KEY_A) };
        let _ = sender.send(registered.is_ok());
        if registered.is_err() {
            return;
        }

        loop {
            let mut message = MSG::default();
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 <= 0 {
                break;
            }
            if message.message == WM_HOTKEY {
                let text = autotype_once();
                if let Ok(mut slot) = shared().message.lock() {
                    *slot = Some(text);
                }
                ctx.request_repaint();
            }
        }
    });

    match receiver.recv() {
        Ok(true) => Ok(()),
        Ok(false) => Err("клавиши Ctrl+Alt+A заняты другой программой".to_string()),
        Err(_) => Err("не удалось запустить поток горячей клавиши".to_string()),
    }
}

/// Отдаёт потоку горячей клавиши актуальную запись. Вызывается из UI.
pub fn set_credentials(credentials: Option<Credentials>) {
    let Ok(mut guard) = shared().credentials.lock() else {
        return;
    };
    let same = match (&*guard, &credentials) {
        (None, None) => true,
        (Some(old), Some(new)) => {
            old.title == new.title && old.username == new.username && *old.password == *new.password
        }
        _ => false,
    };
    if !same {
        // Прежний набор зануляется при замене.
        *guard = credentials;
    }
}

/// Забирает сообщение о последнем автовводе для строки состояния.
pub fn take_message() -> Option<String> {
    shared()
        .message
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// Один автоввод по запросу из UI (кнопка с отсчётом).
/// Возвращает текст для строки состояния.
pub fn run_now() -> String {
    autotype_once()
}

/// Один автоввод: берёт текущую запись и печатает её в активное окно.
fn autotype_once() -> String {
    let credentials = shared()
        .credentials
        .lock()
        .ok()
        .and_then(|slot| slot.clone());

    let Some(credentials) = credentials else {
        return "Автоввод: сначала разблокируйте хранилище и выберите запись (Ctrl+Alt+A)."
            .to_string();
    };

    if credentials.username.is_empty() && credentials.password.is_empty() {
        return format!(
            "У записи «{}» нет логина и пароля для автоввода.",
            credentials.title
        );
    }

    match type_credentials(&credentials.username, &credentials.password) {
        Ok(()) => format!(
            "Логин и пароль введены в активное окно: {}.",
            credentials.title
        ),
        Err(err) => format!("Автоввод не удался: {err}."),
    }
}

/// Печатает данные в активное окно: логин, Tab, пароль, Enter.
fn type_credentials(username: &str, password: &str) -> Result<(), String> {
    if foreground_is_ours() {
        return Err("активно окно «Хранителя» — переключитесь в целевое окно".to_string());
    }

    if !username.is_empty() {
        send_text(username)?;
        send_virtual(VK_TAB)?;
    }
    send_text(password)?;
    send_virtual(VK_RETURN)?;
    Ok(())
}

/// Активное окно принадлежит нашей программе?
fn foreground_is_ours() -> bool {
    unsafe {
        let window = GetForegroundWindow();
        let mut process_id = 0u32;
        GetWindowThreadProcessId(window, Some(&mut process_id as *mut u32));
        process_id == GetCurrentProcessId()
    }
}

fn send_text(text: &str) -> Result<(), String> {
    let mut inputs = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        inputs.push(unicode_input(unit, false));
        inputs.push(unicode_input(unit, true));
    }
    send(&inputs)
}

fn send_virtual(key: VIRTUAL_KEY) -> Result<(), String> {
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[down, up])
}

fn unicode_input(unit: u16, released: bool) -> INPUT {
    let flags = if released {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(
            "Windows отклонила ввод — возможно, активное окно запущено от администратора"
                .to_string(),
        );
    }
    // Короткая пауза, чтобы приложение-адресат успело обработать символы.
    thread::sleep(std::time::Duration::from_millis(15));
    Ok(())
}

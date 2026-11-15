//! Автоввод логина и пароля в активное окно (аналог Auto-Type в KeePass).
//!
//! Горячая клавиша по умолчанию — Ctrl+Alt+P; её можно сменить в настройках.
//! Регистрация живёт на отдельном потоке: WM_HOTKEY приходит в его очередь,
//! поток сам печатает данные через `SendInput` (KEYEVENTF_UNICODE — работают
//! русские пароли и любая раскладка) и просит egui перерисоваться.
//!
//! Почему не в UI-кадре: когда окно свёрнуто, eframe не вызывает `ui()`,
//! а автоввод должен работать именно из свёрнутого приложения.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use eframe::egui;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
    RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_RETURN, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG, PM_NOREMOVE, PeekMessageW,
    WM_HOTKEY,
};
use zeroize::Zeroizing;

/// Идентификатор горячей клавиши — произвольный, лишь бы уникальный в программе.
const HOTKEY_ID: i32 = 0x4A1;
/// Сообщение потоку: перечитать сочетание из `BINDING` и перерегистрироваться.
const WM_REBIND: u32 = 0x8000 + 1;

/// Сочетание клавиш для автоввода.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Виртуальный код клавиши (как в Win32).
    pub key: u32,
}

impl Default for Hotkey {
    fn default() -> Self {
        // Ctrl+Alt+P встречается реже, чем Ctrl+Alt+A у других программ.
        Self {
            ctrl: true,
            alt: true,
            shift: false,
            key: 0x50,
        }
    }
}

impl Hotkey {
    /// Разбирает запись вида «Ctrl+Alt+P» или «ctrl+shift+f2».
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut hotkey = Self {
            ctrl: false,
            alt: false,
            shift: false,
            key: 0,
        };
        let mut has_key = false;

        for part in text.split('+').map(|part| part.trim().to_lowercase()) {
            match part.as_str() {
                "" => {}
                "ctrl" | "control" => hotkey.ctrl = true,
                "alt" => hotkey.alt = true,
                "shift" => hotkey.shift = true,
                name => {
                    if has_key {
                        return Err("в сочетании должна быть ровно одна клавиша".into());
                    }
                    hotkey.key = parse_key(name)?;
                    has_key = true;
                }
            }
        }

        if !has_key {
            return Err("не указана клавиша: например, Ctrl+Alt+P".into());
        }
        if !hotkey.ctrl && !hotkey.alt && !hotkey.shift {
            return Err("нужен хотя бы один модификатор: Ctrl, Alt или Shift".into());
        }
        Ok(hotkey)
    }

    /// Запись для файла настроек и подсказок.
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }

    fn modifiers(self) -> HOT_KEY_MODIFIERS {
        let mut modifiers = MOD_NOREPEAT;
        if self.ctrl {
            modifiers |= MOD_CONTROL;
        }
        if self.alt {
            modifiers |= MOD_ALT;
        }
        if self.shift {
            modifiers |= MOD_SHIFT;
        }
        modifiers
    }
}

fn parse_key(name: &str) -> Result<u32, String> {
    if let Some(number) = name.strip_prefix('f')
        && let Ok(number) = number.parse::<u32>()
        && (1..=24).contains(&number)
    {
        return Ok(0x70 + number - 1);
    }

    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next())
        && ch.is_ascii_alphanumeric()
    {
        return Ok(u32::from(ch.to_ascii_uppercase() as u8));
    }

    Err(format!(
        "не понимаю клавишу «{name}»: подойдут буква, цифра или F1–F24"
    ))
}

fn key_name(key: u32) -> String {
    if (0x70..=0x87).contains(&key) {
        return format!("F{}", key - 0x6F);
    }
    char::from_u32(key)
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| format!("0x{key:02X}"))
}

/// Что печатать по горячей клавише: заголовок для отчёта и сами данные.
#[derive(Clone)]
pub struct Credentials {
    pub title: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

/// Состояние автоввода, которое UI публикует для потока горячей клавиши.
pub enum AutotypeState {
    /// Хранилище закрыто.
    Locked,
    /// Хранилище открыто, но запись не выбрана.
    NoSelection,
    /// Готово к вводу.
    Ready(Credentials),
}

struct Shared {
    state: Mutex<AutotypeState>,
    message: Mutex<Option<String>>,
    binding: Mutex<Hotkey>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        state: Mutex::new(AutotypeState::Locked),
        message: Mutex::new(None),
        binding: Mutex::new(Hotkey::default()),
    })
}

/// Идентификатор потока горячей клавиши — ему адресуются команды переназначения.
static HOTKEY_THREAD: AtomicU32 = AtomicU32::new(0);
/// Текущее сочетание для показа в интерфейсе.
static CURRENT: OnceLock<Mutex<Hotkey>> = OnceLock::new();

fn current() -> &'static Mutex<Hotkey> {
    CURRENT.get_or_init(|| Mutex::new(Hotkey::default()))
}

/// Регистрирует горячую клавишу и запускает поток ожидания нажатия.
pub fn start(ctx: egui::Context, hotkey: Hotkey) -> Result<(), String> {
    *current().lock().expect("мьютекс текущей клавиши") = hotkey;
    if let Ok(mut binding) = shared().binding.lock() {
        *binding = hotkey;
    }

    let (sender, receiver) = std::sync::mpsc::channel();

    thread::spawn(move || {
        // Очередь сообщений потока создаётся при первом обращении к ней.
        let mut message = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        }

        HOTKEY_THREAD.store(current_thread_id(), Ordering::SeqCst);

        let mut active = hotkey;
        let registered = unsafe { RegisterHotKey(None, HOTKEY_ID, active.modifiers(), active.key) };
        let _ = sender.send((registered.is_ok(), active));
        if registered.is_err() {
            // Ждём команд переназначения — вдруг пользователь выберет другую.
            wait_loop(&ctx);
            return;
        }

        loop {
            let mut message = MSG::default();
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 <= 0 {
                break;
            }
            match message.message {
                WM_HOTKEY => {
                    let text = autotype_once();
                    if let Ok(mut slot) = shared().message.lock() {
                        *slot = Some(text);
                    }
                    ctx.request_repaint();
                }
                WM_REBIND => {
                    let next = shared()
                        .binding
                        .lock()
                        .map(|binding| *binding)
                        .unwrap_or(active);
                    if next == active {
                        continue;
                    }
                    unsafe {
                        let _ = UnregisterHotKey(None, HOTKEY_ID);
                        let ok =
                            RegisterHotKey(None, HOTKEY_ID, next.modifiers(), next.key).is_ok();
                        let text = if ok {
                            *current().lock().expect("мьютекс текущей клавиши") = next;
                            active = next;
                            format!("Автоввод: новая комбинация {}", next.label())
                        } else {
                            // Старая уже снята — честно сообщаем, что клавиши нет.
                            format!(
                                "Автоввод: комбинация {} занята другой программой",
                                next.label()
                            )
                        };
                        if let Ok(mut slot) = shared().message.lock() {
                            *slot = Some(text);
                        }
                        ctx.request_repaint();
                    }
                }
                _ => {}
            }
        }
    });

    match receiver.recv() {
        Ok((true, _)) => Ok(()),
        Ok((false, binding)) => Err(format!(
            "клавиши {} заняты другой программой",
            binding.label()
        )),
        Err(_) => Err("не удалось запустить поток горячей клавиши".to_string()),
    }
}

/// Цикл ожидания команд, когда клавиша не зарегистрирована.
fn wait_loop(ctx: &egui::Context) {
    loop {
        let mut message = MSG::default();
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        if message.message == WM_REBIND {
            let next = shared()
                .binding
                .lock()
                .map(|binding| *binding)
                .unwrap_or_default();
            unsafe {
                if RegisterHotKey(None, HOTKEY_ID, next.modifiers(), next.key).is_ok() {
                    *current().lock().expect("мьютекс текущей клавиши") = next;
                    if let Ok(mut slot) = shared().message.lock() {
                        *slot = Some(format!("Автоввод: комбинация {}", next.label()));
                    }
                    ctx.request_repaint();
                    return;
                }
                if let Ok(mut slot) = shared().message.lock() {
                    *slot = Some(format!(
                        "Автоввод: комбинация {} занята другой программой",
                        next.label()
                    ));
                }
                ctx.request_repaint();
            }
        }
    }
}

/// Просит поток перерегистрировать сочетание.
pub fn rebind(hotkey: Hotkey) {
    if let Ok(mut binding) = shared().binding.lock() {
        *binding = hotkey;
    }
    let thread_id = HOTKEY_THREAD.load(Ordering::SeqCst);
    if thread_id != 0 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                thread_id,
                WM_REBIND,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

/// Отдаёт потоку горячей клавиши актуальное состояние. Вызывается из UI.
pub fn set_state(state: AutotypeState) {
    let Ok(mut guard) = shared().state.lock() else {
        return;
    };
    let same = match (&*guard, &state) {
        (AutotypeState::Locked, AutotypeState::Locked) => true,
        (AutotypeState::NoSelection, AutotypeState::NoSelection) => true,
        (AutotypeState::Ready(old), AutotypeState::Ready(new)) => {
            old.title == new.title && old.username == new.username && *old.password == *new.password
        }
        _ => false,
    };
    if !same {
        *guard = state;
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
pub fn run_now() -> String {
    autotype_once()
}

/// Один автоввод: берёт текущую запись и печатает её в активное окно.
fn autotype_once() -> String {
    let state = shared()
        .state
        .lock()
        .map(|guard| match &*guard {
            AutotypeState::Locked => AutotypeState::Locked,
            AutotypeState::NoSelection => AutotypeState::NoSelection,
            AutotypeState::Ready(credentials) => AutotypeState::Ready(credentials.clone()),
        })
        .unwrap_or(AutotypeState::Locked);

    let credentials = match state {
        AutotypeState::Locked => {
            return "Автоввод: хранилище заблокировано. Откройте его мастер-паролем.".to_string();
        }
        AutotypeState::NoSelection => {
            return "Автоввод: выберите запись в списке — печатается выбранная запись.".to_string();
        }
        AutotypeState::Ready(credentials) => credentials,
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
    thread::sleep(std::time::Duration::from_millis(15));
    Ok(())
}

/// Идентификатор текущего потока (Win32).
fn current_thread_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_labels_it_back() {
        let hotkey = Hotkey::parse("ctrl+alt+p").expect("разбор");
        assert_eq!(hotkey, Hotkey::default());
        assert_eq!(hotkey.label(), "Ctrl+Alt+P");
    }

    #[test]
    fn parses_other_modifiers_and_keys() {
        let hotkey = Hotkey::parse("Ctrl+Shift+F2").expect("разбор");
        assert!(hotkey.ctrl && hotkey.shift && !hotkey.alt);
        assert_eq!(hotkey.key, 0x71);
        assert_eq!(hotkey.label(), "Ctrl+Shift+F2");
    }

    #[test]
    fn rejects_invalid_bindings() {
        assert!(Hotkey::parse("P").is_err());
        assert!(Hotkey::parse("Ctrl+Alt").is_err());
        assert!(Hotkey::parse("Ctrl+Alt+pp").is_err());
    }
}

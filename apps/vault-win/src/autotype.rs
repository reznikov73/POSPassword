//! Автоввод логина и пароля в активное окно (аналог Auto-Type в KeePass).
//!
//! Горячая клавиша по умолчанию — Ctrl+Alt+P; её можно сменить в настройках.
//! Регистрация живёт на отдельном потоке: WM_HOTKEY приходит в его очередь,
//! поток сам печатает данные через `SendInput` (KEYEVENTF_UNICODE — работают
//! русские пароли и любая раскладка) и просит egui перерисоваться.
//!
//! Почему не в UI-кадре: когда окно свёрнуто, eframe не вызывает `ui()`,
//! а автоввод должен работать именно из свёрнутого приложения.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use eframe::egui;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
    RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_RETURN, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetMessageW, GetWindowTextW, GetWindowThreadProcessId, MSG,
    PM_NOREMOVE, PeekMessageW, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_HOTKEY,
};
use zeroize::Zeroizing;

/// Идентификатор горячей клавиши автоввода — произвольный, лишь бы уникальный в программе.
const HOTKEY_ID: i32 = 0x4A1;
/// Идентификатор горячей клавиши быстрого доступа.
const QUICK_ID: i32 = 0x4A2;
/// Сообщение потоку: перечитать сочетание из `BINDING` и перерегистрироваться.
const WM_REBIND: u32 = 0x8000 + 1;
/// Заголовок главного окна — по нему окно ищется при открытии быстрого доступа.
const MAIN_TITLE_PREFIX: &str = "Хранитель";

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

/// Сочетание быстрого доступа (как Quick Access у 1Password).
pub fn quick_default() -> Hotkey {
    Hotkey {
        ctrl: true,
        alt: false,
        shift: true,
        key: 0x20, // VK_SPACE
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

    /// Сочетание из нажатия в окне настроек: egui отдаёт клавишу и модификаторы.
    /// Возвращает None, если модификатора нет или такую клавишу не зарегистрировать.
    pub fn from_egui(key: egui::Key, modifiers: egui::Modifiers) -> Option<Self> {
        if !modifiers.ctrl && !modifiers.alt && !modifiers.shift {
            return None;
        }
        Some(Self {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            key: vk_from_egui(key)?,
        })
    }
}

fn parse_key(name: &str) -> Result<u32, String> {
    if name == "space" || name == "пробел" {
        return Ok(0x20);
    }
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
    if key == 0x20 {
        return "Space".to_string();
    }
    if (0x70..=0x87).contains(&key) {
        return format!("F{}", key - 0x6F);
    }
    char::from_u32(key)
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| format!("0x{key:02X}"))
}

/// Виртуальный код Win32 для клавиши egui. Поддерживаем то, что умеет
/// разбор текста: буквы, цифры и F1–F20 (их хватает для глобальных сочетаний).
fn vk_from_egui(key: egui::Key) -> Option<u32> {
    use egui::Key;
    Some(match key {
        Key::A => 0x41,
        Key::B => 0x42,
        Key::C => 0x43,
        Key::D => 0x44,
        Key::E => 0x45,
        Key::F => 0x46,
        Key::G => 0x47,
        Key::H => 0x48,
        Key::I => 0x49,
        Key::J => 0x4A,
        Key::K => 0x4B,
        Key::L => 0x4C,
        Key::M => 0x4D,
        Key::N => 0x4E,
        Key::O => 0x4F,
        Key::P => 0x50,
        Key::Q => 0x51,
        Key::R => 0x52,
        Key::S => 0x53,
        Key::T => 0x54,
        Key::U => 0x55,
        Key::V => 0x56,
        Key::W => 0x57,
        Key::X => 0x58,
        Key::Y => 0x59,
        Key::Z => 0x5A,
        Key::Num0 => 0x30,
        Key::Num1 => 0x31,
        Key::Num2 => 0x32,
        Key::Num3 => 0x33,
        Key::Num4 => 0x34,
        Key::Num5 => 0x35,
        Key::Num6 => 0x36,
        Key::Num7 => 0x37,
        Key::Num8 => 0x38,
        Key::Num9 => 0x39,
        Key::F1 => 0x70,
        Key::F2 => 0x71,
        Key::F3 => 0x72,
        Key::F4 => 0x73,
        Key::F5 => 0x74,
        Key::F6 => 0x75,
        Key::F7 => 0x76,
        Key::F8 => 0x77,
        Key::F9 => 0x78,
        Key::F10 => 0x79,
        Key::F11 => 0x7A,
        Key::F12 => 0x7B,
        Key::F13 => 0x7C,
        Key::F14 => 0x7D,
        Key::F15 => 0x7E,
        Key::F16 => 0x7F,
        Key::F17 => 0x80,
        Key::F18 => 0x81,
        Key::F19 => 0x82,
        Key::F20 => 0x83,
        Key::Space => 0x20,
        _ => return None,
    })
}

/// Что печатать по горячей клавише: заголовок для отчёта и сами данные.
#[derive(Clone)]
pub struct Credentials {
    pub title: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

/// Целевое окно для быстрого доступа: то, что было активно в момент нажатия.
#[derive(Clone)]
pub struct QuickRequest {
    /// Заголовок целевого окна (для подсказки и выбора записи). None — если
    /// в момент нажатия активно было окно «Хранителя».
    pub title: Option<String>,
    /// Системный дескриптор целевого окна (0 — если целевого окна нет).
    pub hwnd: isize,
}

/// Какой из двух горячих клавиш меняем сочетание.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Autotype,
    Quick,
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
    /// Запрос на открытие быстрого доступа из потока горячих клавиш.
    quick_request: Mutex<Option<QuickRequest>>,
    binding: Mutex<Hotkey>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        state: Mutex::new(AutotypeState::Locked),
        message: Mutex::new(None),
        quick_request: Mutex::new(None),
        binding: Mutex::new(Hotkey::default()),
    })
}

/// Идентификатор потока горячей клавиши — ему адресуются команды переназначения.
static HOTKEY_THREAD: AtomicU32 = AtomicU32::new(0);
/// Зарегистрирована ли клавиша автоввода прямо сейчас.
static READY: AtomicBool = AtomicBool::new(false);
/// Зарегистрирована ли клавиша быстрого доступа прямо сейчас.
static QUICK_READY: AtomicBool = AtomicBool::new(false);
/// Текущее сочетание для показа в интерфейсе.
static CURRENT: OnceLock<Mutex<Hotkey>> = OnceLock::new();
static QUICK_CURRENT: OnceLock<Mutex<Hotkey>> = OnceLock::new();

fn current() -> &'static Mutex<Hotkey> {
    CURRENT.get_or_init(|| Mutex::new(Hotkey::default()))
}

fn quick_current() -> &'static Mutex<Hotkey> {
    QUICK_CURRENT.get_or_init(|| Mutex::new(quick_default()))
}

/// Запрос на смену сочетания: поток отвечает, получилось ли зарегистрировать.
struct RebindRequest {
    slot: Slot,
    hotkey: Hotkey,
    reply: std::sync::mpsc::Sender<Result<(), String>>,
}

fn rebind_slot_storage() -> &'static Mutex<Option<RebindRequest>> {
    static SLOT: OnceLock<Mutex<Option<RebindRequest>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Готова ли горячая клавиша автоввода к работе. UI спрашивает это каждый кадр,
/// поэтому подсказка в строке состояния не врёт после смены сочетания.
pub fn is_ready() -> bool {
    READY.load(Ordering::SeqCst)
}

/// Готова ли горячая клавиша быстрого доступа.
pub fn is_quick_ready() -> bool {
    QUICK_READY.load(Ordering::SeqCst)
}

/// Забирает запрос на быстрый доступ: целевое окно, нажатое пользователем.
pub fn take_quick_request() -> Option<QuickRequest> {
    shared()
        .quick_request
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// Регистрирует обе горячие клавиши и запускает поток ожидания нажатия.
/// Сочетание быстрого доступа может не встать — это не мешает автовводу.
pub fn start(ctx: egui::Context, hotkey: Hotkey, quick: Hotkey) -> Result<(), String> {
    *current().lock().expect("мьютекс текущей клавиши") = hotkey;
    *quick_current().lock().expect("мьютекс клавиши доступа") = quick;
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

        // Если сочетание занято, поток всё равно остаётся живым: пользователь
        // выберет другое в настройках, и регистрация произойдёт на месте.
        let mut active = None;
        let registered = unsafe { RegisterHotKey(None, HOTKEY_ID, hotkey.modifiers(), hotkey.key) };
        if registered.is_ok() {
            active = Some(hotkey);
            READY.store(true, Ordering::SeqCst);
        }

        let mut quick_active = None;
        let quick_registered =
            unsafe { RegisterHotKey(None, QUICK_ID, quick.modifiers(), quick.key) };
        if quick_registered.is_ok() {
            quick_active = Some(quick);
            QUICK_READY.store(true, Ordering::SeqCst);
        }
        let _ = sender.send((registered.is_ok(), quick_registered.is_ok(), hotkey));

        loop {
            let mut message = MSG::default();
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 <= 0 {
                break;
            }
            match message.message {
                WM_HOTKEY => {
                    if message.wParam.0 == QUICK_ID as usize {
                        quick_pressed(&ctx);
                    } else {
                        let text = autotype_once();
                        if let Ok(mut slot) = shared().message.lock() {
                            *slot = Some(text);
                        }
                    }
                    ctx.request_repaint();
                }
                WM_REBIND => {
                    let request = rebind_slot_storage()
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take());
                    let Some(request) = request else {
                        continue;
                    };
                    let outcome = match request.slot {
                        Slot::Autotype => {
                            apply_binding(HOTKEY_ID, &mut active, request.hotkey, &READY)
                        }
                        Slot::Quick => {
                            apply_binding(QUICK_ID, &mut quick_active, request.hotkey, &QUICK_READY)
                        }
                    };
                    let label = match request.slot {
                        Slot::Autotype => "Автоввод",
                        Slot::Quick => "Быстрый доступ",
                    };
                    if let Ok(mut slot) = shared().message.lock() {
                        *slot = Some(match &outcome {
                            Ok(()) => {
                                format!("{label}: новая комбинация {}", request.hotkey.label())
                            }
                            Err(error) => format!("{label}: {error}"),
                        });
                    }
                    let _ = request.reply.send(outcome);
                    ctx.request_repaint();
                }
                _ => {}
            }
        }

        READY.store(false, Ordering::SeqCst);
        QUICK_READY.store(false, Ordering::SeqCst);
    });

    match receiver.recv() {
        Ok((true, _, _)) => Ok(()),
        Ok((false, true, binding)) => Err(format!(
            "клавиши {} заняты другой программой — работает только быстрый доступ",
            binding.label()
        )),
        Ok((false, false, binding)) => Err(format!(
            "клавиши {} заняты другой программой",
            binding.label()
        )),
        Err(_) => Err("не удалось запустить поток горячей клавиши".to_string()),
    }
}

/// Снимает прежнюю регистрацию и ставит новую. Если новая занята, старая
/// возвращается на место — иначе пользователь остался бы вообще без автоввода.
fn apply_binding(
    id: i32,
    active: &mut Option<Hotkey>,
    requested: Hotkey,
    ready: &AtomicBool,
) -> Result<(), String> {
    let previous = *active;
    if previous == Some(requested) && ready.load(Ordering::SeqCst) {
        return Ok(());
    }

    unsafe {
        let _ = UnregisterHotKey(None, id);
    }
    ready.store(false, Ordering::SeqCst);
    *active = None;

    let registered = unsafe { RegisterHotKey(None, id, requested.modifiers(), requested.key) };
    if registered.is_ok() {
        *active = Some(requested);
        let slot_current = if id == QUICK_ID {
            quick_current()
        } else {
            current()
        };
        *slot_current.lock().expect("мьютекс текущей клавиши") = requested;
        ready.store(true, Ordering::SeqCst);
        return Ok(());
    }

    let mut restored = false;
    if let Some(previous) = previous
        && unsafe { RegisterHotKey(None, id, previous.modifiers(), previous.key) }.is_ok()
    {
        *active = Some(previous);
        restored = true;
        ready.store(true, Ordering::SeqCst);
    }

    Err(if restored {
        format!(
            "комбинация {} занята другой программой — осталась {}",
            requested.label(),
            previous.map(Hotkey::label).unwrap_or_default()
        )
    } else {
        format!("комбинация {} занята другой программой", requested.label())
    })
}

/// Просит поток перерегистрировать сочетание автоввода и дожидается ответа.
/// Ошибка означает, что комбинация не встала (старая при этом сохраняется).
pub fn rebind(hotkey: Hotkey) -> Result<(), String> {
    rebind_in_thread(Slot::Autotype, hotkey)
}

/// То же для сочетания быстрого доступа.
pub fn rebind_quick(hotkey: Hotkey) -> Result<(), String> {
    rebind_in_thread(Slot::Quick, hotkey)
}

fn rebind_in_thread(slot: Slot, hotkey: Hotkey) -> Result<(), String> {
    if slot == Slot::Autotype
        && let Ok(mut binding) = shared().binding.lock()
    {
        *binding = hotkey;
    }
    let thread_id = HOTKEY_THREAD.load(Ordering::SeqCst);
    if thread_id == 0 {
        return Err("поток горячих клавиш ещё не запущен".to_string());
    }

    let (reply, answer) = std::sync::mpsc::channel();
    if let Ok(mut request) = rebind_slot_storage().lock() {
        *request = Some(RebindRequest {
            slot,
            hotkey,
            reply,
        });
    }
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
            thread_id,
            WM_REBIND,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }

    match answer.recv_timeout(Duration::from_millis(800)) {
        Ok(result) => result,
        Err(_) => Err("поток автоввода не ответил на смену комбинации".to_string()),
    }
}

/// Один автоввод по таймеру: не зависит от кадров UI, поэтому работает
/// и из свёрнутого окна (в свёрнутом виде egui не вызывает `ui()`).
pub fn run_after(delay: Duration, ctx: egui::Context) {
    thread::spawn(move || {
        thread::sleep(delay);
        let text = autotype_once();
        if let Ok(mut slot) = shared().message.lock() {
            *slot = Some(text);
        }
        ctx.request_repaint();
    });
}

/// Вводит выбранную в быстром доступе запись в заранее запомненное окно.
/// Работает из отдельного потока: окно-мишень получает фокус, получает
/// небольшую фору на переключение, затем в него печатается логин и пароль.
pub fn run_quick(
    ctx: egui::Context,
    target_hwnd: isize,
    username: String,
    password: Zeroizing<String>,
    title: String,
) {
    thread::spawn(move || {
        if target_hwnd != 0 {
            // Целевое окно помнит нажатие — сначала возвращаем ему фокус.
            unsafe {
                let _ = SetForegroundWindow(HWND(target_hwnd as *mut core::ffi::c_void));
            }
            thread::sleep(Duration::from_millis(250));
        }
        let text = match type_credentials(&username, &password) {
            Ok(()) => format!("Логин и пароль введены в активное окно: {title}."),
            Err(err) => format!("Автоввод не удался: {err}."),
        };
        if let Ok(mut slot) = shared().message.lock() {
            *slot = Some(text);
        }
        ctx.request_repaint();
    });
}

/// Нажата клавиша быстрого доступа: запоминаем целевое окно (то, что было
/// активно до нас), поднимаем главное окно «Хранителя» и просим перерисовки.
fn quick_pressed(ctx: &egui::Context) {
    let window = unsafe { GetForegroundWindow() };
    let own_window = window_pid(window) == std::process::id();
    let request = QuickRequest {
        title: (!own_window).then(|| window_title(window)),
        hwnd: if own_window { 0 } else { window.0 as isize },
    };
    if let Ok(mut slot) = shared().quick_request.lock() {
        *slot = Some(request);
    }
    restore_main_window();
    ctx.request_repaint();
}

/// Заголовок окна.
fn window_title(window: HWND) -> String {
    let mut buffer = [0u16; 512];
    let len = unsafe { GetWindowTextW(window, &mut buffer) };
    if len <= 0 {
        return String::new();
    }
    let len = len as usize;
    String::from_utf16_lossy(&buffer[..len.min(buffer.len())])
}

/// Процесс-владелец окна.
fn window_pid(window: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(window, Some(&mut pid as *mut u32));
    }
    pid
}

/// Поднимает главное окно «Хранителя»: разворачивает, если свёрнуто,
/// и переводит на передний план. Вызывается с потока горячих клавиш.
fn restore_main_window() {
    struct Collector {
        found: Option<HWND>,
    }

    // SAFETY: lParam указывает на структуру в стеке текущего вызова —
    // перечисление синхронное, указатель живёт до возврата из EnumWindows.
    unsafe extern "system" fn callback(window: HWND, lparam: LPARAM) -> windows::core::BOOL {
        // SAFETY: указатель получен от нас же в вызове EnumWindows ниже.
        let collector = unsafe { &mut *(lparam.0 as *mut Collector) };
        if collector.found.is_some() {
            return windows::core::BOOL(1);
        }
        if window_pid(window) == std::process::id() {
            let title = window_title(window);
            if title.starts_with(MAIN_TITLE_PREFIX) {
                collector.found = Some(window);
            }
        }
        windows::core::BOOL(1)
    }

    let mut collector = Collector { found: None };
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut collector as *mut Collector as isize),
        );
        if let Some(window) = collector.found {
            // SW_RESTORE и разворачивает свёрнутое окно, и активирует обычное.
            let _ = ShowWindow(window, SW_RESTORE);
            let _ = SetForegroundWindow(window);
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
    // Горячая клавиша приходит в момент нажатия, когда Ctrl/Alt ещё зажаты.
    // Если начать печатать сразу, Windows отдаёт символы как сочетания
    // (Ctrl+U, Alt+P…), и в чужом окне не появляется ничего.
    wait_for_modifiers_release(Duration::from_millis(1200));

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

/// Ждёт, пока пользователь отпустит Ctrl, Alt, Shift и Win.
fn wait_for_modifiers_release(timeout: Duration) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    let keys = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        let pressed = keys
            .iter()
            .any(|key| unsafe { (GetAsyncKeyState(key.0 as i32) as u16) & 0x8000 != 0 });
        if !pressed {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Небольшая пауза, чтобы целевое окно успело получить key-up.
    thread::sleep(Duration::from_millis(40));
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

    #[test]
    fn builds_binding_from_a_pressed_combination() {
        let modifiers = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let hotkey = Hotkey::from_egui(egui::Key::F2, modifiers).expect("сочетание");
        assert_eq!(hotkey.label(), "Ctrl+Shift+F2");
        assert_eq!(hotkey, Hotkey::parse("ctrl+shift+f2").expect("разбор"));
    }

    #[test]
    fn ignores_presses_without_modifier_or_unsupported_keys() {
        assert!(Hotkey::from_egui(egui::Key::P, egui::Modifiers::NONE).is_none());
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert!(Hotkey::from_egui(egui::Key::ArrowLeft, ctrl).is_none());
    }

    #[test]
    fn parses_space_hotkey_for_quick_access() {
        let hotkey = Hotkey::parse("ctrl+shift+space").expect("разбор");
        assert_eq!(hotkey, quick_default());
        assert_eq!(hotkey.label(), "Ctrl+Shift+Space");
        // Русское написание тоже понимаем — и в обоих направлениях.
        assert_eq!(
            Hotkey::parse("Ctrl+Shift+Пробел").expect("разбор по-русски"),
            quick_default()
        );
        // Сочетания двух клавиш должны различаться.
        assert_ne!(quick_default(), Hotkey::default());
    }

    #[test]
    fn rebinding_replaces_the_registered_combination() {
        let first = Hotkey::parse("Ctrl+Alt+F9").expect("разбор первой");
        let second = Hotkey::parse("Ctrl+Alt+F10").expect("разбор второй");
        let quick_first = Hotkey::parse("Ctrl+Alt+F11").expect("разбор первой быстрой");
        let quick_second = Hotkey::parse("Ctrl+Alt+F12").expect("разбор второй быстрой");
        let _ = start(egui::Context::default(), first, quick_first);

        rebind(second).expect("новая комбинация должна встать");
        assert_eq!(current().lock().expect("мьютекс").label(), "Ctrl+Alt+F10");
        assert!(is_ready());

        rebind_quick(quick_second).expect("новая быстрая комбинация должна встать");
        assert_eq!(
            quick_current().lock().expect("мьютекс").label(),
            "Ctrl+Alt+F12"
        );
        assert!(is_quick_ready());

        rebind(first).expect("возврат на прежнюю комбинацию");
        assert_eq!(current().lock().expect("мьютекс").label(), "Ctrl+Alt+F9");
    }
}

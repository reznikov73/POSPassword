//! Значок в области уведомлений и его меню.
//!
//! Живёт на своём потоке: создаёт скрытое окно, ставит значок
//! `Shell_NotifyIconW` и крутит цикл сообщений. Сами действия не выполняет —
//! шлёт команды в UI, потому что окно и его видимостью управляет egui.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ExtractIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, IDI_APPLICATION, LoadIconW, MF_SEPARATOR, MF_STRING, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, SW_RESTORE, SetForegroundWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{HSTRING, w};

/// Что выбрал пользователь в меню значка.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    QuickAccess,
    Lock,
    Settings,
    Exit,
}

/// Идентификаторы пунктов меню.
const CMD_OPEN: usize = 1;
const CMD_QUICK: usize = 2;
const CMD_LOCK: usize = 3;
const CMD_SETTINGS: usize = 4;
const CMD_EXIT: usize = 5;

/// Сообщение значку (клики, меню).
const WM_TRAY: u32 = WM_APP + 7;
/// Идентификатор значка.
const TRAY_ID: u32 = 1;

static SENDER: OnceLock<Mutex<Option<Sender<TrayCommand>>>> = OnceLock::new();
static ICON_ADDED: Mutex<bool> = Mutex::new(false);
/// Дескриптор главного окна: из него значка показываем окно сами,
/// не дожидаясь кадра egui (спрятанное окно кадров не получает).
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

fn sender() -> &'static Mutex<Option<Sender<TrayCommand>>> {
    SENDER.get_or_init(|| Mutex::new(None))
}

/// Ставит значок и возвращает канал команд. `None` — значок не создался
/// (нет оболочки, запрещено политикой); тогда трей просто не работает.
pub fn start(main_hwnd: isize) -> Option<Receiver<TrayCommand>> {
    MAIN_HWND.store(main_hwnd, std::sync::atomic::Ordering::SeqCst);
    let (tx, rx) = channel();
    if let Ok(mut slot) = sender().lock() {
        *slot = Some(tx);
    }

    let (ready, receiver) = channel();
    std::thread::spawn(move || unsafe {
        if let Err(err) = run(ready) {
            eprintln!("tray: не удалось создать значок: {err}");
        }
    });

    match receiver.recv_timeout(std::time::Duration::from_millis(1500)) {
        Ok(true) => Some(rx),
        _ => None,
    }
}

unsafe fn run(ready: Sender<bool>) -> windows::core::Result<()> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("HranitelTrayWindow");

    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    unsafe {
        let _ = RegisterClassW(&wc);
    }
    eprintln!(
        "tray: класс зарегистрирован, ошибка {:?}",
        unsafe { windows::Win32::Foundation::GetLastError() }
    );

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Хранитель"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
    };
    let Ok(hwnd) = hwnd else {
        eprintln!("tray: CreateWindowExW не удался: {:?}", hwnd.err());
        let _ = ready.send(false);
        return Ok(());
    };
    eprintln!("tray: окно создано {hwnd:?}");

    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: unsafe { tray_icon() },
        ..Default::default()
    };
    let tip: Vec<u16> = "Хранитель — пароли".encode_utf16().collect();
    let tip_len = tip.len().min(data.szTip.len() - 1);
    data.szTip[..tip_len].copy_from_slice(&tip[..tip_len]);

    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
    eprintln!("tray: Shell_NotifyIcon NIM_ADD = {added}");
    if let Ok(mut flag) = ICON_ADDED.lock() {
        *flag = added;
    }
    let _ = ready.send(added);

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    if let Ok(flag) = ICON_ADDED.lock()
        && *flag
    {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }
    Ok(())
}

/// Иконка приложения из exe; если не вышло — стандартная.
unsafe fn tray_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    if let Ok(path) = std::env::current_exe() {
        let icon = unsafe { ExtractIconW(None, &HSTRING::from(path.as_os_str()), 0) };
        if !icon.is_invalid() && icon.0 as isize > 1 {
            return icon;
        }
    }
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TRAY => {
            let event = (lparam.0 as u32) & 0xFFFF;
            match event {
                WM_LBUTTONUP => {
                    show_main_window();
                    send_command(TrayCommand::Open);
                }
                WM_RBUTTONUP => unsafe { show_menu(hwnd) },
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            let command = match id {
                CMD_OPEN => Some(TrayCommand::Open),
                CMD_QUICK => Some(TrayCommand::QuickAccess),
                CMD_LOCK => Some(TrayCommand::Lock),
                CMD_SETTINGS => Some(TrayCommand::Settings),
                CMD_EXIT => Some(TrayCommand::Exit),
                _ => None,
            };
            if let Some(command) = command {
                if !matches!(command, TrayCommand::Exit) {
                    show_main_window();
                }
                send_command(command);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn send_command(command: TrayCommand) {
    if let Ok(slot) = sender().lock()
        && let Some(sender) = slot.as_ref()
    {
        let _ = sender.send(command);
    }
}

/// Показывает главное окно силами трея: egui о спрятанном окне не знает,
/// пока не получит кадр, а кадр без видимого окна не приходит.
fn show_main_window() {
    let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_TOOLWINDOW,
    };
    let window = HWND(hwnd as *mut core::ffi::c_void);
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        SetWindowLongPtrW(window, GWL_EXSTYLE, style & !(WS_EX_TOOLWINDOW.0 as isize));
        let _ = SetWindowPos(
            window,
            None,
            100,
            100,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(window, SW_RESTORE);
        let _ = SetForegroundWindow(window);
    }
}

/// Контекстное меню у курсора.
unsafe fn show_menu(hwnd: HWND) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("Открыть"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUICK, w!("Быстрый доступ"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_LOCK, w!("Заблокировать"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, w!("Настройки"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("Выход"));

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // Окно должно быть активным, иначе меню не закроется по клику мимо.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        if chosen.0 != 0 {
            let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(chosen.0 as usize), LPARAM(0));
        }
    }
}

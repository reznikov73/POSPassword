//! Утилиты Windows: окно, автозапуск, один экземпляр, проверка адресов.

/// Хэндл мьютекса текущего экземпляра. Нужен только чтобы не закрывать его.
pub(crate) static INSTANCE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Пытается взять именованный мьютекс. `true` — другой экземпляр уже работает.
pub(crate) fn other_instance_running() -> bool {
    use windows::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, GetLastError, SetLastError, WIN32_ERROR,
    };
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;

    // SAFETY: имя — статическая строка, атрибуты не нужны.
    unsafe {
        // Код последней ошибки может быть сталым от прошлых вызовов.
        SetLastError(WIN32_ERROR(0));
        match CreateMutexW(None, true, w!("Hranitel.SingleInstance")) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    true
                } else {
                    // Хэндл нигде не закрываем: он живёт до конца процесса
                    // и держит мьютекс всё время работы.
                    INSTANCE.store(handle.0 as isize, std::sync::atomic::Ordering::Relaxed);
                    false
                }
            }
            Err(_) => false,
        }
    }
}

/// Дескриптор главного окна: нужен трею, чтобы показать окно из своего потока.
pub(crate) fn main_window_hwnd(cc: &eframe::CreationContext<'_>) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
        _ => 0,
    }
}

pub(crate) fn hide_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowLongPtrW, SetWindowPos, WS_EX_TOOLWINDOW,
    };
    // Убираем кнопку с панели задач и Alt+Tab и уводим окно за экран:
    // прятать через ShowWindow нельзя — egui каждый кадр показывает его снова.
    let window = windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void);
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        SetWindowLongPtrW(window, GWL_EXSTYLE, style | WS_EX_TOOLWINDOW.0 as isize);
        let _ = SetWindowPos(
            window,
            None,
            -32000,
            -32000,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Возвращает окно на экран и в панель задач.
pub(crate) fn show_window_now(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SW_RESTORE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_TOOLWINDOW,
    };
    let window = windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void);
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

/// Включён ли автозапуск на самом деле: смотрим ключ Run, а не settings.txt.
/// Иначе после установщика (или ручной правки реестра) галочка врёт.
pub(crate) fn autostart_enabled() -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::HSTRING;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "Hranitel";

    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            &HSTRING::from(VALUE),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS
}

/// Включает или выключает автозапуск при входе в Windows (ключ Run).
pub(crate) fn set_autostart(enabled: bool) -> Result<(), String> {
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
        RegOpenKeyExW, RegSetValueExW,
    };
    use windows::core::HSTRING;

    const SUCCESS: WIN32_ERROR = WIN32_ERROR(0);
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "Hranitel";

    unsafe {
        let mut key = HKEY::default();
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            Some(0),
            KEY_SET_VALUE,
            &mut key,
        );
        if opened != SUCCESS {
            return Err(format!("не удалось открыть ключ автозапуска: {opened:?}"));
        }

        let result = if enabled {
            let exe = std::env::current_exe().map_err(|err| err.to_string())?;
            let command = format!("\"{}\" --tray", exe.display());
            let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(key, &HSTRING::from(VALUE), Some(0), REG_SZ, Some(bytes))
        } else {
            // Значения не было — это не ошибка.
            let status = RegDeleteValueW(key, &HSTRING::from(VALUE));
            if status == SUCCESS || status == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                SUCCESS
            } else {
                status
            }
        };
        let _ = RegCloseKey(key);

        if result == SUCCESS {
            Ok(())
        } else {
            Err(format!("реестр вернул {result:?}"))
        }
    }
}

/// Имя сайта без зоны: fialkaspb.ru → fialkaspb.
pub(crate) fn site_label(domain: &str) -> String {
    domain.split('.').next().unwrap_or(domain).to_string()
}

/// Пароль из браузера отдаём только когда в запросе есть адрес страницы и
/// запись действительно относится к этому сайту. Пустой адрес — отказ: иначе
/// локальный процесс мог бы получить секрет вообще без диалога.
pub(crate) fn credentials_url_ok(url: &str, item_matches: bool) -> bool {
    !url.trim().is_empty() && item_matches
}

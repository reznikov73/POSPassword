//! UI Automation: адресная строка браузера и тип поля в фокусе.
//!
//! Нужно для быстрого доступа: по адресу открытой страницы палитра сразу
//! подставляет сайт в поиск, а ввод выбирает порядок «логин → пароль» или
//! «пароль → логин» по тому, куда поставлен курсор.
//!
//! Chromium (Chrome, Edge, Яндекс Браузер) отдаёт дерево доступности лениво:
//! первый запрос может занять до секунды, поэтому вызовы делаются из фонового
//! потока, а при любой ошибке возвращается `None` — вызывающий код откатывается
//! к прежнему поведению.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants,
    UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_ValuePatternId,
};

/// Что сейчас в фокусе — от этого зависит порядок ввода.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusedField {
    /// Обычное текстовое поле: логин, почта, телефон.
    Text,
    /// Поле пароля.
    Password,
}

/// Порядок печати в форму.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillOrder {
    /// Курсор в текстовом поле: логин, Tab, пароль.
    LoginThenPassword,
    /// Курсор в поле пароля: пароль, Shift+Tab, логин.
    PasswordThenLogin,
}

/// Выбирает порядок ввода по сфокусированному полю. Неизвестное поле —
/// как раньше, логин и пароль (это самый частый сценарий).
pub fn fill_order(focused: Option<FocusedField>) -> FillOrder {
    match focused {
        Some(FocusedField::Password) => FillOrder::PasswordThenLogin,
        _ => FillOrder::LoginThenPassword,
    }
}

/// Адрес открытой страницы из адресной строки окна браузера.
/// `None` — окно не браузер или UI Automation недоступен.
pub fn address_bar_url(hwnd: isize) -> Option<String> {
    if hwnd == 0 {
        return None;
    }
    let automation = automation()?;
    let window =
        unsafe { automation.ElementFromHandle(HWND(hwnd as *mut core::ffi::c_void)) }.ok()?;
    let condition = unsafe {
        automation.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_EditControlTypeId.0),
        )
    }
    .ok()?;
    let found = unsafe { window.FindAll(TreeScope_Descendants, &condition) }.ok()?;
    let count = unsafe { found.Length() }.unwrap_or(0);

    for index in 0..count {
        let Ok(element) = (unsafe { found.GetElement(index) }) else {
            continue;
        };
        let name = unsafe { element.CurrentName() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let automation_id = unsafe { element.CurrentAutomationId() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        if !looks_like_address_bar(&name, &automation_id) {
            continue;
        }
        let Ok(pattern) = (unsafe {
            element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        }) else {
            continue;
        };
        let Ok(value) = (unsafe { pattern.CurrentValue() }) else {
            continue;
        };
        let text = value.to_string();
        let text = text.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

/// Тип поля в фокусе активного окна. `None` — фокус не в редактируемом поле
/// (страница целиком, кнопка, адресная строка) или UIA недоступен.
pub fn focused_field() -> Option<FocusedField> {
    let automation = automation()?;
    let element = unsafe { automation.GetFocusedElement() }.ok()?;
    let control_type = unsafe { element.CurrentControlType() }.ok()?;
    if control_type != UIA_EditControlTypeId {
        return None;
    }
    let password = unsafe { element.CurrentIsPassword() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    Some(if password {
        FocusedField::Password
    } else {
        FocusedField::Text
    })
}

/// COM нужен потоку для UIA. eframe уже мог инициализировать его в другом
/// режиме — тогда вернётся ошибка, но сам COM уже работает, и UIA доступен.
fn automation() -> Option<IUIAutomation> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// Похоже ли поле на адресную строку браузера. Названия локальные, поэтому
/// проверяем несколько вариантов; «просто адрес» не подходит — на странице
/// бывает поле «Адрес доставки».
fn looks_like_address_bar(name: &str, automation_id: &str) -> bool {
    let name = name.to_lowercase();
    let id = automation_id.to_lowercase();
    const HINTS: [&str; 8] = [
        "адресная строка",
        "поиск или ввод веб-адреса",
        "введите адрес",
        "address and search bar",
        "search or enter web address",
        "address bar",
        "omnibox",
        "адрес сайта",
    ];
    if HINTS.iter().any(|hint| name.contains(hint)) {
        return true;
    }
    id.contains("address") || id.contains("omnibox")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_address_bars_of_common_browsers() {
        assert!(looks_like_address_bar("Адресная строка", ""));
        assert!(looks_like_address_bar("Address and search bar", ""));
        assert!(looks_like_address_bar("Search or enter web address", ""));
        assert!(looks_like_address_bar("Поиск или ввод веб-адреса", ""));
        assert!(looks_like_address_bar("", "addressEditBox"));
        assert!(looks_like_address_bar("", "omnibox"));
    }

    #[test]
    fn does_not_take_page_fields_for_the_address_bar() {
        assert!(!looks_like_address_bar("Адрес доставки", ""));
        assert!(!looks_like_address_bar("Логин", "login"));
        assert!(!looks_like_address_bar("Пароль", "password"));
        assert!(!looks_like_address_bar("", ""));
    }

    #[test]
    fn typing_order_depends_on_the_focused_field() {
        assert_eq!(
            fill_order(Some(FocusedField::Password)),
            FillOrder::PasswordThenLogin
        );
        assert_eq!(
            fill_order(Some(FocusedField::Text)),
            FillOrder::LoginThenPassword
        );
        // Фокус неизвестен — привычный порядок.
        assert_eq!(fill_order(None), FillOrder::LoginThenPassword);
    }

    /// Ручная проверка: с открытым браузером запустить
    /// `cargo test -p vault-win -- --ignored --nocapture`.
    /// Печатает адреса всех верхних окон Яндекс Браузера.
    #[test]
    #[ignore]
    fn prints_address_bars_of_running_browsers() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

        unsafe extern "system" fn callback(
            window: HWND,
            _param: windows::Win32::Foundation::LPARAM,
        ) -> windows::core::BOOL {
            let mut buffer = [0u16; 512];
            let len = unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(window, &mut buffer)
            };
            let title = String::from_utf16_lossy(&buffer[..len.max(0) as usize]);
            if title.to_lowercase().contains("браузер")
                || title.contains("Google Chrome")
                || title.contains("Microsoft Edge")
            {
                match address_bar_url(window.0 as isize) {
                    Some(url) => println!("окно {title:?} → {url}"),
                    None => println!("окно {title:?} → адрес не прочитан"),
                }
            }
            windows::core::BOOL(1)
        }

        unsafe {
            let _ = EnumWindows(Some(callback), windows::Win32::Foundation::LPARAM(0));
        }
    }
}

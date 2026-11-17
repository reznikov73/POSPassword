//! Системные диалоги Windows: выбор файла и путь из буфера обмена.
//!
//! Нужны потому, что файл, скопированный в Проводнике, лежит в буфере как
//! список файлов (CF_HDROP), а не как текст: обычная вставка в поле пути
//! ничего не даёт. Отдельная кнопка читает именно этот формат.

use std::path::PathBuf;

use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::{PWSTR, w};

/// Открывает системный диалог «Открыть файл». `None` — пользователь отменил.
pub fn pick_file() -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrFilter: w!(
            "CSV и JSON\0*.csv;*.json;*.txt\0CSV (*.csv)\0*.csv\0JSON (*.json)\0*.json\0Все файлы\0*.*\0\0"
        ),
        lpstrTitle: w!("Файл для импорта"),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };

    // SAFETY: буфер и структура живут до конца вызова, память под путь достаточно велика.
    let chosen = unsafe { GetOpenFileNameW(&mut dialog) };
    if !chosen.as_bool() {
        return None;
    }
    Some(PathBuf::from(read_wide(&buffer)))
}

/// Открывает системный диалог «Сохранить как» с подставленным именем файла.
pub fn save_file(suggested: &str) -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    for (index, unit) in suggested.encode_utf16().take(buffer.len() - 1).enumerate() {
        buffer[index] = unit;
    }

    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrFilter: w!("CSV (*.csv)\0*.csv\0JSON (*.json)\0*.json\0Все файлы\0*.*\0\0"),
        lpstrTitle: w!("Куда сохранить экспорт"),
        Flags: OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR,
        ..Default::default()
    };

    // SAFETY: буфер и структура живут до конца вызова.
    let chosen = unsafe { GetSaveFileNameW(&mut dialog) };
    if !chosen.as_bool() {
        return None;
    }
    Some(PathBuf::from(read_wide(&buffer)))
}

/// Путь к файлу, скопированному в Проводнике (Ctrl+C по файлу).
pub fn clipboard_file() -> Option<PathBuf> {
    // SAFETY: буфер обмена открывается на время чтения и закрывается в конце.
    unsafe {
        OpenClipboard(None).ok()?;
        let format = u32::from(CF_HDROP.0);
        let result = (|| {
            IsClipboardFormatAvailable(format).ok()?;
            let handle = GetClipboardData(format).ok()?;
            let drop = HDROP(handle.0);
            let length = DragQueryFileW(drop, 0, None);
            if length == 0 {
                return None;
            }
            let mut buffer = vec![0u16; length as usize + 1];
            let written = DragQueryFileW(drop, 0, Some(&mut buffer));
            if written == 0 {
                return None;
            }
            Some(PathBuf::from(read_wide(&buffer)))
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Текст из буфера обмена — на случай, когда скопирован сам путь строкой.
pub fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut clipboard| clipboard.get_text().ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Строка из нуль-терминированного буфера.
fn read_wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Диалог сохранения резервной копии хранилища (Ф-5): фильтр .opvault.
pub fn save_vault_copy(suggested: &str) -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    for (index, unit) in suggested.encode_utf16().take(buffer.len() - 1).enumerate() {
        buffer[index] = unit;
    }

    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrFilter: w!("Хранилище (*.opvault)\0*.opvault\0Все файлы\0*.*\0\0"),
        lpstrTitle: w!("Куда сохранить резервную копию"),
        Flags: OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR,
        ..Default::default()
    };

    // SAFETY: буфер и структура живут до конца вызова.
    let chosen = unsafe { GetSaveFileNameW(&mut dialog) };
    if !chosen.as_bool() {
        return None;
    }
    Some(PathBuf::from(read_wide(&buffer)))
}
#[cfg(test)]
mod tests {
    use super::read_wide;

    #[test]
    fn reads_until_nul() {
        let buffer = [b'C' as u16, b':' as u16, b'\\' as u16, 0, b'X' as u16];
        assert_eq!(read_wide(&buffer), "C:\\");
    }
}

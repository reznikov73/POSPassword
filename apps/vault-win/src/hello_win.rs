//! Windows Hello и DPAPI: системная часть разблокировки.
//!
//! Криптография обёртки ключа живёт в `vault-core`; здесь только вызовы
//! Windows: DPAPI защищает ключ обёртки для текущей учётной записи, а
//! `UserConsentVerifier` просит подтвердить присутствие человека.
//!
//! DPAPI-блоб привязан к учётной записи Windows и этой машине: на другом
//! компьютере или под другим пользователем он не разворачивается.

use std::ffi::c_void;

use vault_core::KEY_LEN;
use windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{HSTRING, PCWSTR};
use zeroize::Zeroizing;

/// Защищает ключ обёртки DPAPI для текущего пользователя.
pub fn dpapi_protect(key: &[u8; KEY_LEN]) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: key.len() as u32,
        pbData: key.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: входной блоб указывает на живой буфер ключа; система заполняет
    // выходной блоб, память освобождаем через LocalFree.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|err| format!("DPAPI не смогла защитить ключ: {err}"))?;

        let blob = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut c_void)));
        Ok(blob)
    }
}

/// Снимает защиту DPAPI. Ошибка означает другую учётную запись, другую
/// машину или повреждённый блоб.
pub fn dpapi_unprotect(blob: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: входной блоб указывает на живой буфер; выходной заполняется
    // системой и освобождается через LocalFree после копирования.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| {
            "Windows не смогла развернуть ключ: другая учётная запись, другая машина или повреждённый файл".to_string()
        })?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let key = if bytes.len() == KEY_LEN {
            let mut key = Zeroizing::new([0u8; KEY_LEN]);
            key.copy_from_slice(bytes);
            Ok(key)
        } else {
            Err("DPAPI вернула ключ неверной длины".to_string())
        };

        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut c_void)));
        key
    }
}

/// Проверяет, доступен ли Windows Hello на этой машине (Б-7).
///
/// Вызывается перед включением опции, чтобы кнопка не обещала того, чего
/// нет. Блокирующий вызов (WinRT): только из фонового потока, не из UI.
pub fn check_availability() -> Result<(), String> {
    // WinRT требует инициализированную квартиру; для рабочего потока — MTA.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let operation = UserConsentVerifier::CheckAvailabilityAsync()
        .map_err(|err| format!("Windows Hello: {err}"))?;
    let availability = operation
        .join()
        .map_err(|err| format!("Windows Hello: {err}"))?;

    match availability {
        UserConsentVerifierAvailability::Available => Ok(()),
        UserConsentVerifierAvailability::DeviceBusy => {
            Err("устройство Windows Hello занято".into())
        }
        UserConsentVerifierAvailability::DeviceNotPresent => {
            Err("устройство Windows Hello не найдено".into())
        }
        UserConsentVerifierAvailability::DisabledByPolicy => {
            Err("Windows Hello отключён политикой".into())
        }
        UserConsentVerifierAvailability::NotConfiguredForUser => {
            Err("Windows Hello не настроен для этой учётной записи".into())
        }
        _ => Err("Windows Hello недоступен".into()),
    }
}

/// Просит подтверждение присутствия (Windows Hello).
///
/// Блокирует вызывающий поток: среда выполнения WinRT ждёт диалог. Вызывать
/// только из фонового потока, не из UI-потока egui.
pub fn request_consent() -> Result<(), String> {
    // WinRT требует инициализированную квартиру; для рабочего потока — MTA.
    // Повторная инициализация или чужая квартира не мешают вызову.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let operation = UserConsentVerifier::RequestVerificationAsync(&HSTRING::from(
        "Подтвердите вход в «Хранитель»",
    ))
    .map_err(|err| format!("Windows Hello: {err}"))?;

    let result = operation
        .join()
        .map_err(|err| format!("Windows Hello: {err}"))?;

    if result == UserConsentVerificationResult::Verified {
        Ok(())
    } else {
        Err(result_message(result).to_string())
    }
}

/// Текст причины отказа — пользователю понятно, что делать дальше.
fn result_message(result: UserConsentVerificationResult) -> &'static str {
    match result {
        UserConsentVerificationResult::Canceled => "подтверждение отменено",
        UserConsentVerificationResult::DeviceNotPresent => "устройство Windows Hello не найдено",
        UserConsentVerificationResult::NotConfiguredForUser => {
            "Windows Hello не настроен для этой учётной записи"
        }
        UserConsentVerificationResult::DisabledByPolicy => "Windows Hello отключён политикой",
        UserConsentVerificationResult::DeviceBusy => "устройство Windows Hello занято",
        UserConsentVerificationResult::RetriesExhausted => "слишком много неудачных попыток",
        _ => "подтверждение не получено",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::generate_wrapping_key;

    #[test]
    fn dpapi_roundtrip() {
        let key = generate_wrapping_key();
        let blob = dpapi_protect(&key).expect("защита DPAPI");
        assert_ne!(
            blob.as_slice(),
            &key[..],
            "блоб не должен совпадать с ключом"
        );

        let restored = dpapi_unprotect(&blob).expect("снятие защиты");
        assert_eq!(&restored[..], &key[..]);
    }

    #[test]
    fn dpapi_rejects_garbage() {
        assert!(dpapi_unprotect(b"not a dpapi blob").is_err());
    }
}

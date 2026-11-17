//! Настройки программы: settings.txt, каталог приложения.

use std::path::PathBuf;

use crate::*;

pub(crate) fn settings_path() -> PathBuf {
    app_dir().join("settings.txt")
}

/// Настройки программы: тема, сочетания клавиш, путь к хранилищу и режим окна.
#[derive(Clone)]
pub(crate) struct Settings {
    pub(crate) theme: Theme,
    pub(crate) hotkey: autotype::Hotkey,
    pub(crate) quick_hotkey: autotype::Hotkey,
    pub(crate) vault_path: Option<String>,
    /// Разрешено ли расширению браузера обращаться к хранилищу.
    pub(crate) browser: bool,
    /// Закрытие окна сворачивает в трей, а не выходит из программы.
    pub(crate) tray: bool,
    /// Запуск при входе в Windows (свёрнутым в трей).
    pub(crate) autostart: bool,
}

pub(crate) fn load_settings() -> Settings {
    let mut settings = Settings {
        theme: Theme::Light,
        hotkey: autotype::Hotkey::default(),
        quick_hotkey: autotype::quick_default(),
        vault_path: None,
        browser: true,
        tray: true,
        autostart: false,
    };
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return settings;
    };
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line == "dark" {
            settings.theme = Theme::Dark;
            continue;
        }
        if line == "light" {
            settings.theme = Theme::Light;
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        match name.trim() {
            "theme" => {
                settings.theme = if value.trim() == "dark" {
                    Theme::Dark
                } else {
                    Theme::Light
                };
            }
            "hotkey" => {
                if let Ok(hotkey) = autotype::Hotkey::parse(value.trim()) {
                    settings.hotkey = hotkey;
                }
            }
            "quickkey" => {
                if let Ok(hotkey) = autotype::Hotkey::parse(value.trim()) {
                    settings.quick_hotkey = hotkey;
                }
            }
            "browser" => {
                settings.browser = value.trim() != "off";
            }
            "tray" => {
                settings.tray = value.trim() != "off";
            }
            "autostart" => {
                settings.autostart = value.trim() == "on";
            }
            "path" => {
                let path = value.trim();
                if !path.is_empty() {
                    settings.vault_path = Some(path.to_string());
                }
            }
            _ => {}
        }
    }
    settings
}

pub(crate) fn save_settings(settings: &Settings) {
    let dir = app_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let theme_value = match settings.theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    let browser_value = if settings.browser { "on" } else { "off" };
    let tray_value = if settings.tray { "on" } else { "off" };
    let autostart_value = if settings.autostart { "on" } else { "off" };
    let mut text = format!(
        "theme={theme_value}\nhotkey={}\nquickkey={}\nbrowser={browser_value}\ntray={tray_value}\nautostart={autostart_value}\n",
        settings.hotkey.label().to_lowercase(),
        settings.quick_hotkey.label().to_lowercase()
    );
    // Последний открытый файл: при следующем запуске поле уже заполнено.
    if let Some(path) = settings.vault_path.as_deref() {
        let path = path.trim();
        if !path.is_empty() {
            text.push_str(&format!("path={path}\n"));
        }
    }
    let _ = std::fs::write(dir.join("settings.txt"), text);
}

pub(crate) fn app_dir() -> PathBuf {
    // В некоторых окружениях APPDATA не задан; тогда собираем путь от профиля.
    let base = std::env::var("APPDATA")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|profile| format!("{profile}\\AppData\\Roaming"))
        })
        .unwrap_or_else(|| ".".to_string());

    PathBuf::from(base).join("Hranitel")
}

pub(crate) fn default_vault_path() -> String {
    app_dir().join("vault.opvault").display().to_string()
}

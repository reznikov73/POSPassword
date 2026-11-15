//! Окно-мишень для проверки автоввода «Хранителя».
//!
//! Это не настоящий вход: программа ничего не проверяет и никуда не отправляет.
//! Она показывает, что именно попало в поля «Логин» и «Пароль», — удобно
//! убедиться, что горячая клавиша напечатала правильную последовательность
//! (логин, Tab, пароль, Enter), включая кириллицу. Сочетание берётся из
//! настроек «Хранителя», чтобы подсказка не расходилась с делом.

// В релизе не открываем консоль рядом с окном.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

const TITLE: &str = "Проверка входа";
const DEFAULT_HOTKEY: &str = "Ctrl+Alt+P";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 460.0])
            .with_min_inner_size([440.0, 360.0])
            .with_title(TITLE),
        ..Default::default()
    };
    eframe::run_native(TITLE, options, Box::new(|cc| Ok(Box::new(Probe::new(cc)))))
}

struct Probe {
    login: String,
    password: String,
    show_password: bool,
    focus_login: bool,
    submitted: Option<(String, String)>,
    submissions: u32,
    /// Сочетание из настроек «Хранителя» — показывается в подсказке.
    hotkey: String,
    hotkey_checked: Instant,
}

impl Probe {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        Self {
            login: String::new(),
            password: String::new(),
            show_password: false,
            focus_login: true,
            submitted: None,
            submissions: 0,
            hotkey: configured_hotkey(),
            hotkey_checked: Instant::now(),
        }
    }
}

/// Путь к настройкам «Хранителя». В некоторых окружениях APPDATA пуст,
/// поэтому второй вариант — путь от профиля пользователя.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var("APPDATA")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| format!("{value}\\AppData\\Roaming"))
        })?;
    Some(PathBuf::from(base).join("Hranitel").join("settings.txt"))
}

/// Сочетание из settings.txt; если файла нет — значение по умолчанию.
fn configured_hotkey() -> String {
    let Some(path) = settings_path() else {
        return DEFAULT_HOTKEY.to_string();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return DEFAULT_HOTKEY.to_string();
    };
    for line in text.lines() {
        let Some(value) = line.trim().strip_prefix("hotkey=") else {
            continue;
        };
        let pretty = pretty_hotkey(value.trim());
        if !pretty.is_empty() {
            return pretty;
        }
    }
    DEFAULT_HOTKEY.to_string()
}

/// «ctrl+alt+p» → «Ctrl+Alt+P», «ctrl+shift+f2» → «Ctrl+Shift+F2».
fn pretty_hotkey(value: &str) -> String {
    value
        .split('+')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let mut chars = part.chars();
            let first = chars.next()?;
            let rest: String = chars.collect();
            if first.eq_ignore_ascii_case(&'f')
                && !rest.is_empty()
                && rest.chars().all(|c| c.is_ascii_digit())
            {
                return Some(format!("F{rest}"));
            }
            Some(format!("{}{rest}", first.to_ascii_uppercase()))
        })
        .collect::<Vec<_>>()
        .join("+")
}

impl eframe::App for Probe {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Настройки «Хранителя» могли измениться при открытом окне —
        // перечитываем раз в две секунды, чтобы подсказка не врала.
        if self.hotkey_checked.elapsed() >= Duration::from_secs(2) {
            self.hotkey_checked = Instant::now();
            self.hotkey = configured_hotkey();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));

        egui::Frame::NONE
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.heading("Проверка автоввода");
                ui.label(
                    egui::RichText::new(format!(
                        "Выберите запись в «Хранителе», щёлкните в поле логина здесь и нажмите {}.",
                        self.hotkey
                    ))
                    .weak()
                    .small(),
                );
                ui.add_space(10.0);

                let mut submitted = false;
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::Grid::new("probe_grid")
                        .num_columns(2)
                        .spacing([10.0, 10.0])
                        .show(ui, |ui| {
                            ui.label("Логин");
                            let login = ui.add(
                                egui::TextEdit::singleline(&mut self.login)
                                    .desired_width(360.0)
                                    .hint_text("сюда должен прийти логин"),
                            );
                            if self.focus_login {
                                login.request_focus();
                                self.focus_login = false;
                            }
                            if login.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                submitted = true;
                            }
                            ui.end_row();

                            ui.label("Пароль");
                            ui.horizontal(|ui| {
                                let edit = ui.add(
                                    egui::TextEdit::singleline(&mut self.password)
                                        .password(!self.show_password)
                                        .desired_width(300.0)
                                        .hint_text("сюда должен прийти пароль"),
                                );
                                if edit.lost_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                {
                                    submitted = true;
                                }
                                let toggle = if self.show_password {
                                    "Скрыть"
                                } else {
                                    "Показать"
                                };
                                if ui.small_button(toggle).clicked() {
                                    self.show_password = !self.show_password;
                                }
                            });
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Войти").clicked() {
                            submitted = true;
                        }
                        if ui.button("Очистить").clicked() {
                            self.login.clear();
                            self.password.clear();
                            self.submitted = None;
                            self.focus_login = true;
                        }
                    });
                });

                if submitted {
                    self.submitted = Some((self.login.clone(), self.password.clone()));
                    self.submissions += 1;
                }

                ui.add_space(12.0);
                ui.label(egui::RichText::new("Что пришло в поля").strong());
                match &self.submitted {
                    None => {
                        ui.label(egui::RichText::new("Ещё не отправлялось.").weak());
                    }
                    Some((login, password)) => {
                        ui.label(format!("Логин: «{login}»"));
                        let shown = if self.show_password {
                            password.clone()
                        } else {
                            "•".repeat(password.chars().count())
                        };
                        ui.label(format!(
                            "Пароль: {shown} ({} символов)",
                            password.chars().count()
                        ));
                        if !password.is_ascii() {
                            ui.label(
                                egui::RichText::new(
                                    "В пароле есть не-ASCII символы — Unicode-ввод работает.",
                                )
                                .small()
                                .weak(),
                            );
                        }
                    }
                }

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Сочетание берётся из настроек «Хранителя»: {}. Отправок: {} · в полях: логин {} симв., пароль {} симв.",
                        self.hotkey,
                        self.submissions,
                        self.login.chars().count(),
                        self.password.chars().count()
                    ))
                    .small()
                    .weak(),
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettifies_saved_binding() {
        assert_eq!(pretty_hotkey("ctrl+alt+p"), "Ctrl+Alt+P");
        assert_eq!(pretty_hotkey("ctrl+shift+f2"), "Ctrl+Shift+F2");
        assert_eq!(pretty_hotkey(""), "");
    }
}

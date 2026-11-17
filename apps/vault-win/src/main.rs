//! Оконный клиент «Хранитель» на eframe/egui.
//!
//! Вся криптография и работа с файлом — в `vault-core`; здесь только UI,
//! буфер обмена с автоочисткой и автоблокировка. Устройство оконного
//! приложения: `state.rs` (состояние), `theme.rs` (оформление),
//! `settings.rs` (настройки), `ui/` (экраны) и `dialogs/` (диалоги).

// В релизе не открываем консольное окно рядом с интерфейсом: оно забирает
// фокус и клавиши (включая F1). В отладочной сборке консоль остаётся для логов.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autotype;
mod browser;
mod dialogs;
mod filedialog;
mod hello_win;
mod help;
mod settings;
mod state;
mod theme;
mod tray;
mod ui;
mod uia;
mod winutil;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui;
use uuid::Uuid;
// Глоб-импорт: всё ядро доступно модулям приложения через `use crate::*`.
use vault_core::*;
use zeroize::{Zeroize, Zeroizing};

pub(crate) use settings::*;
pub(crate) use state::*;
pub(crate) use theme::*;
pub(crate) use ui::*;
pub(crate) use winutil::*;

pub(crate) const APP_TITLE: &str = "Хранитель";
pub(crate) const AUTO_LOCK: Duration = Duration::from_secs(5 * 60);
pub(crate) const CLIPBOARD_TTL: Duration = Duration::from_secs(30);

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    // --tray: старт свёрнутым (автозапуск), --quiet: не показывать окно
    // "уже запущен" (так программу поднимает мост из браузера).
    let start_hidden = args.iter().any(|arg| arg == "--tray");
    let quiet = args.iter().any(|arg| arg == "--quiet");

    // Один экземпляр: два окна путают автоввод — горячую клавишу держит только одно.
    if other_instance_running() {
        if !quiet {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    MB_ICONINFORMATION, MB_OK, MessageBoxW,
                };
                use windows::core::w;
                MessageBoxW(
                    None,
                    w!(
                        "«Хранитель» уже запущен в другом окне.\n\nЗакройте старое окно и запустите это снова — иначе автоввод будет работать из старого экземпляра."
                    ),
                    w!("Хранитель"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }
        std::process::exit(0);
    }

    // Геометрия окна из настроек (И-7): размер и позиция прошлого запуска.
    // Позиция за пределами экрана поправится после первого кадра — тут
    // размер монитора ещё неизвестен.
    let settings = load_settings();
    let mut viewport = egui::ViewportBuilder::default()
        .with_min_inner_size([760.0, 520.0])
        .with_visible(!start_hidden)
        .with_title(format!("{APP_TITLE} {}", env!("CARGO_PKG_VERSION")));
    viewport = match settings.size {
        Some([w, h]) => viewport.with_inner_size([w.max(760.0), h.max(520.0)]),
        None => viewport.with_inner_size([1020.0, 680.0]),
    };
    if let Some([x, y]) = settings.pos {
        viewport = viewport.with_position(egui::Pos2::new(x, y));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

impl eframe::App for App {
    // Фон окна берём из темы, иначе eframe заливает его чёрным.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.start_hidden {
            self.start_hidden = false;
            hide_window(self.main_hwnd);
            // Окно всё равно успевает мелькнуть: прячем ещё раз через полсекунды.
            self.hide_after = Some(Instant::now() + Duration::from_millis(500));
        }
        if let Some(at) = self.hide_after {
            if Instant::now() >= at {
                self.hide_after = None;
                hide_window(self.main_hwnd);
            } else {
                ui.ctx().request_repaint_after(at - Instant::now());
            }
        }
        self.handle_close_request(ui.ctx());
        self.handle_tray_commands(ui.ctx());
        self.remember_target_window();
        self.tick(ui.ctx());
        self.track_activity(ui.ctx());
        self.track_window_geometry(ui.ctx());
        self.poll_kdf_measure();

        // Запросы расширения браузера: разбор и диалоги поверх остального.
        self.poll_browser_jobs(ui.ctx());
        self.browser_dialog(ui.ctx());

        // F1 открывает и закрывает справку в любом месте программы. Событие
        // разбираем напрямую: так работает и при необычных модификаторах.
        // Ctrl+H — запасной вариант для ноутбуков, где F1 занята мультимедиа.
        let f1_pressed = !self.hotkey_capture
            && !self.quick_capture
            && (ui.ctx().input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::F1,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                })
            }) || ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::H)));
        if f1_pressed {
            self.help_open = !self.help_open;
        }

        // Ctrl+F — курсор в поиск; Esc закрывает диалоги и очищает поиск.
        autotype::set_state(self.autotype_state());
        if let Some(message) = autotype::take_message() {
            if message.starts_with("Автоввод") {
                // Нечего печатать — показываем окно, чтобы сообщение точно увидели.
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.status = message.clone();
            self.autotype_notice = Some((message, Instant::now()));
        }

        // Файл, перетащенный в окно, сразу открывает диалог импорта.
        let dropped = ui.ctx().input(|input| {
            input
                .raw
                .dropped_files
                .first()
                .map(|file| file.path().to_path_buf())
        });
        if let Some(path) = dropped {
            self.import_path = path.display().to_string();
            self.import_open = true;
            self.import_result = None;
            self.status = format!("Файл для импорта: {}", path.display());
        }
        if self.screen.is_main()
            && !self.hotkey_capture
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F))
        {
            self.focus_search = true;
        }
        if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
            && !self.help_open
            && !self.hotkey_capture
            && !self.quick_capture
        {
            if self.quick_access.is_some() {
                self.quick_access = None;
            } else if self.quick_open {
                self.quick_open = false;
                self.quick_text.zeroize();
                self.quick_password.zeroize();
            } else if self.gen_open {
                self.gen_open = false;
                self.gen_result.zeroize();
            } else if self.desc_open {
                self.desc_open = false;
            } else if self.password_open {
                self.password_open = false;
            } else if self.export_open {
                self.export_open = false;
            } else if self.audit_open {
                self.audit_open = false;
            } else if let Some(query) = self.screen.query_mut()
                && !query.is_empty()
            {
                query.clear();
            }
        }

        let status_height = 34.0;
        let content_height = (ui.available_height() - status_height).max(0.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        // Занимаем всю высоту, чтобы строка состояния была внизу окна.
                        ui.set_min_height(ui.available_height());
                        if self.screen.is_main() {
                            self.main_ui(ui);
                        } else {
                            self.unlock_ui(ui);
                        }
                    });
            },
        );

        self.status_bar(ui);

        help::show(ui.ctx(), &mut self.help_open);
        self.settings_dialog(ui.ctx());
        self.autotype_overlay(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_credentials_require_a_site() {
        assert!(!credentials_url_ok("", true));
        assert!(!credentials_url_ok("   ", true));
        assert!(!credentials_url_ok("https://x.ru", false));
        assert!(credentials_url_ok("https://x.ru", true));
    }

    #[test]
    fn browser_site_domain_is_empty_for_garbage() {
        assert!(App::site_domain("").is_empty());
        assert!(App::site_domain("https://").is_empty());
        assert_eq!(
            App::site_domain("https://lk.domclick.ru/login"),
            "domclick.ru"
        );
    }

    #[test]
    fn fresh_phrase_sample_is_not_regenerated() {
        // После генерации отметка равна языку — образец стабилен.
        assert!(!phrase_sample_stale(
            GenMode::Phrase,
            Some(Wordlist::Russian),
            Wordlist::Russian
        ));
        assert!(!phrase_sample_stale(
            GenMode::Phrase,
            Some(Wordlist::English),
            Wordlist::English
        ));
    }

    #[test]
    fn changing_language_marks_sample_as_stale() {
        assert!(phrase_sample_stale(
            GenMode::Phrase,
            Some(Wordlist::English),
            Wordlist::Russian
        ));
        assert!(phrase_sample_stale(
            GenMode::Phrase,
            None,
            Wordlist::Russian
        ));
        // В режиме случайных символов отметка языка не важна.
        assert!(!phrase_sample_stale(
            GenMode::Chars,
            None,
            Wordlist::Russian
        ));
    }
}

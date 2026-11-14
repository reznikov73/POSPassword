//! Окно-мишень для проверки автоввода «Хранителя».
//!
//! Это не настоящий вход: программа ничего не проверяет и никуда не отправляет.
//! Она показывает, что именно попало в поля «Логин» и «Пароль», — удобно
//! убедиться, что Ctrl+Alt+A напечатал правильную последовательность
//! (логин, Tab, пароль, Enter), включая кириллицу.

// В релизе не открываем консоль рядом с окном.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

const TITLE: &str = "Проверка входа";

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
        }
    }
}

impl eframe::App for Probe {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.heading("Проверка автоввода");
                ui.label(
                    egui::RichText::new(
                        "Выберите запись в «Хранителе», щёлкните в поле логина здесь и нажмите Ctrl+Alt+A.",
                    )
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
                        "Отправок: {} · в полях: логин {} симв., пароль {} симв.",
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

//! Экран входа: файл хранилища, мастер-пароль, Windows Hello.

use crate::*;

impl App {
    pub(crate) fn unlock_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(36.0);
        ui.vertical_centered(|ui| {
            ui.heading(APP_TITLE);
            ui.label(
                egui::RichText::new("Локальное хранилище паролей · Windows")
                    .weak()
                    .small(),
            );
            ui.add_space(22.0);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(440.0);
                ui.add_space(4.0);

                ui.label("Файл хранилища");
                ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(f32::INFINITY));
                ui.add_space(8.0);

                ui.label("Мастер-пароль");
                let mut empty = String::new();
                let password = self.screen.unlock_password_mut().unwrap_or(&mut empty);
                let password_edit = ui.add(
                    egui::TextEdit::singleline(password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                // Браузер или трей попросили доступ — сразу ставим курсор сюда.
                if self.screen.focus_password() {
                    password_edit.request_focus();
                    self.screen.set_focus_password(false);
                }

                if self.screen.unlock_mode() == Some(Mode::Create) {
                    ui.add_space(8.0);
                    ui.label("Повторите мастер-пароль");
                    let mut empty = String::new();
                    let confirm = self.screen.unlock_confirm_mut().unwrap_or(&mut empty);
                    ui.add(
                        egui::TextEdit::singleline(confirm)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    );
                }

                ui.add_space(12.0);
                let mode = self.screen.unlock_mode().unwrap_or(Mode::Open);
                ui.horizontal(|ui| {
                    let action = match mode {
                        Mode::Open => "Открыть",
                        Mode::Create => "Создать хранилище",
                    };
                    let clicked = ui.button(action).clicked()
                        || (password_edit.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if clicked {
                        self.submit_unlock();
                    }

                    match mode {
                        Mode::Open => {
                            if ui.button("Создать новый…").clicked() {
                                self.screen.set_unlock_mode(Mode::Create);
                                self.error = None;
                                if let Some(password) = self.screen.unlock_password_mut() {
                                    password.zeroize();
                                }
                                if let Some(confirm) = self.screen.unlock_confirm_mut() {
                                    confirm.zeroize();
                                }
                            }
                        }
                        Mode::Create => {
                            if ui.button("Назад").clicked() {
                                self.screen.set_unlock_mode(Mode::Open);
                                self.error = None;
                                if let Some(confirm) = self.screen.unlock_confirm_mut() {
                                    confirm.zeroize();
                                }
                            }
                        }
                    }
                });

                if self.screen.hello_pending_is_some() {
                    ui.add_space(8.0);
                    ui.label("Ожидание подтверждения Windows Hello…");
                    ui.ctx().request_repaint_after(Duration::from_millis(200));
                } else if self.hello_sidecar().is_some() {
                    ui.add_space(8.0);
                    if ui
                        .button("Войти через Windows Hello")
                        .on_hover_text("Разблокировка без мастер-пароля")
                        .clicked()
                    {
                        self.start_hello_unlock(ui.ctx());
                    }
                }

                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error);
                }
                ui.add_space(4.0);
            });

            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(
                    "Argon2id (64 МиБ, 3 итерации) + XChaCha20-Poly1305. Пароль не покидает машину.",
                )
                .weak()
                .small(),
            );
            ui.add_space(8.0);
            if ui
                .small_button(format!("Тема: {}", self.theme.label()))
                .clicked()
            {
                self.theme = self.theme.toggled();
                self.theme.apply(ui.ctx());
                save_settings(&self.settings_snapshot());
            }
            if ui.small_button("Справка (F1)").clicked() {
                self.help_open = true;
            }
            if ui.small_button("Настройки").clicked() {
                self.settings_open = true;
                self.hotkey_draft = self.hotkey.label();
                self.hotkey_error = None;
                self.hotkey_capture = false;
                self.hotkey_focus = true;
            }
        });
    }
}

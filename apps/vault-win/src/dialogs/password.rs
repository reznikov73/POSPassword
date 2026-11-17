//! Диалог смены мастер-пароля.

use crate::*;

impl App {
    /// Диалог смены мастер-пароля.
    pub(crate) fn password_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if !self.password_open {
            return;
        }

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Сменить мастер-пароль")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Текущий пароль");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password_current)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.label("Новый пароль (не короче 8 символов)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password_new)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.label("Повторите новый пароль");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password_confirm)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );

                if let Some(error) = &self.password_error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Сменить").clicked() {
                        submit = true;
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Файл будет перешифрован новым ключом, вход через Windows Hello отключится.",
                    )
                    .weak()
                    .small(),
                );
            });

        if cancel || !open {
            self.password_open = false;
            self.password_current.zeroize();
            self.password_new.zeroize();
            self.password_confirm.zeroize();
            self.password_error = None;
            return;
        }

        if submit {
            self.password_error = None;
            if self.password_new != self.password_confirm {
                self.password_error = Some("Новые пароли не совпадают.".into());
            } else if self.password_new.chars().count() < 8 {
                self.password_error = Some("Новый пароль короче 8 символов.".into());
            } else {
                // Текущий пароль проверяем настоящей расшифровкой файла.
                match Vault::open(vault.path(), &self.password_current) {
                    Err(_) => {
                        self.password_error = Some("Текущий пароль неверен.".into());
                    }
                    Ok(probe) => {
                        drop(probe);
                        match vault.change_password(&self.password_new) {
                            Ok(()) => {
                                self.status = "Мастер-пароль изменён. Вход через Windows Hello отключён — включите заново.".to_string();
                                self.password_open = false;
                                self.password_current.zeroize();
                                self.password_new.zeroize();
                                self.password_confirm.zeroize();
                            }
                            Err(err) => {
                                self.password_error =
                                    Some(format!("Не удалось сменить пароль: {err}"));
                            }
                        }
                    }
                }
            }
        }
    }
}

//! Быстрая вставка: разбор скопированного логина и пароля.

use crate::*;

impl App {
    /// Быстрая вставка: разбирает скопированный логин и пароль в новую запись.
    pub(crate) fn quick_add_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if !self.quick_open {
            return;
        }

        let mut open = true;
        let mut paste = false;
        let mut parse = false;
        let mut create = false;
        let mut cancel = false;

        egui::Window::new("Быстрая вставка логина и пароля")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    "Вставьте скопированное: «логин пароль», login:password, «сайт логин пароль», адрес вида https://ivan:secret@site.ru или строки «Логин: …» и «Пароль: …».",
                );
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.quick_text)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("например: mail.ru ivan@example.com Pa$$w0rd"),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Взять из буфера обмена").clicked() {
                        paste = true;
                    }
                    if ui.button("Разобрать").clicked() {
                        parse = true;
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                egui::Grid::new("quick_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Название");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_title)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label("Логин");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_username)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label("Пароль");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_password)
                                .desired_width(f32::INFINITY)
                                .password(true),
                        );
                        ui.end_row();

                        ui.label("Сайт");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_url)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                    });

                ui.add_space(8.0);
                let can_create =
                    !self.quick_username.trim().is_empty() || !self.quick_password.is_empty();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_create, egui::Button::new("Создать запись"))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                    if !can_create {
                        ui.label(
                            egui::RichText::new("Вставьте данные и нажмите «Разобрать»")
                                .weak()
                                .small(),
                        );
                    }
                });
            });

        if paste {
            if let Ok(mut cb) = arboard::Clipboard::new()
                && let Ok(text) = cb.get_text()
                && !text.trim().is_empty()
            {
                self.quick_text = text;
                parse = true;
            } else {
                self.status = "В буфере обмена нет текста.".to_string();
            }
        }

        if parse {
            let parsed = parse_credentials(&self.quick_text);
            self.quick_title = parsed.title;
            self.quick_username = parsed.username;
            self.quick_password = parsed.password;
            self.quick_url = parsed.url;
        }

        if create {
            let title = if self.quick_title.trim().is_empty() {
                "Новая запись"
            } else {
                self.quick_title.trim()
            };
            let mut item = Item::new(ItemKind::Login, title);
            item.username = self.quick_username.trim().to_string();
            item.password = self.quick_password.clone();
            item.url = self.quick_url.trim().to_string();
            item.notes = auto_description(&item);
            match vault.add(item.clone()) {
                Ok(id) => {
                    self.screen.set_selected(Some(id));
                    self.status = format!("Запись «{}» создана.", item.title);
                    self.quick_open = false;
                    self.quick_text.zeroize();
                    self.quick_password.zeroize();
                }
                Err(err) => self.status = format!("Не удалось создать запись: {err}"),
            }
        }

        if cancel {
            open = false;
        }
        if !open {
            self.quick_open = false;
            self.quick_text.zeroize();
            self.quick_password.zeroize();
        }
    }
}

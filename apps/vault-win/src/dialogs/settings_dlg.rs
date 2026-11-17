//! Диалог настроек.

use crate::*;

impl App {
    /// Диалог настроек: сочетание клавиш автоввода.
    pub(crate) fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let mut open = true;
        let mut save = false;
        let mut cancel = false;

        egui::Window::new("Настройки")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Сочетание клавиш для автоввода");
                ui.horizontal(|ui| {
                    if self.hotkey_capture {
                        ui.label(
                            egui::RichText::new("Нажмите новое сочетание…")
                                .strong()
                                .size(16.0),
                        );
                    } else {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.hotkey_draft)
                                .hint_text("Ctrl+Alt+P")
                                .desired_width(220.0),
                        );
                        if self.hotkey_focus {
                            edit.request_focus();
                            self.hotkey_focus = false;
                        }
                    }
                    let label = if self.hotkey_capture {
                        "Отменить запись"
                    } else {
                        "Записать сочетание"
                    };
                    if ui.button(label).clicked() {
                        self.hotkey_capture = !self.hotkey_capture;
                        self.hotkey_error = None;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Можно просто нажать сочетание целиком или вписать его текстом: Ctrl+Alt+P, Ctrl+Shift+F2. Нужен хотя бы один модификатор.",
                    )
                    .weak()
                    .small(),
                );
                ui.label(
                    egui::RichText::new(format!("Сейчас: {}", self.hotkey.label()))
                        .small()
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(if autotype::is_ready() {
                        "Клавиша зарегистрирована и работает."
                    } else {
                        "Клавиша сейчас не работает — комбинацию заняла другая программа."
                    })
                    .small()
                    .weak(),
                );

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label("Сочетание клавиш быстрого доступа");
                ui.horizontal(|ui| {
                    if self.quick_capture {
                        ui.label(
                            egui::RichText::new("Нажмите новое сочетание…")
                                .strong()
                                .size(16.0),
                        );
                    } else {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_draft)
                                .hint_text("Ctrl+Shift+Space")
                                .desired_width(220.0),
                        );
                    }
                    let label = if self.quick_capture {
                        "Отменить запись"
                    } else {
                        "Записать сочетание"
                    };
                    if ui.button(label).clicked() {
                        self.quick_capture = !self.quick_capture;
                        self.quick_error = None;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Нажатая комбинация из любого окна поднимает палитру выбора записи: фильтруйте, Enter — ввести логин и пароль в прежнее окно.",
                    )
                    .weak()
                    .small(),
                );
                ui.label(
                    egui::RichText::new(format!("Сейчас: {}", self.quick_hotkey.label()))
                        .small()
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(if autotype::is_quick_ready() {
                        "Клавиша быстрого доступа зарегистрирована и работает."
                    } else {
                        "Клавиша быстрого доступа сейчас не работает — комбинацию заняла другая программа."
                    })
                    .small()
                    .weak(),
                );

                if let Some(error) = &self.quick_error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(200, 60, 60), error);
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                let mut enabled = self.browser_enabled;
                ui.checkbox(&mut enabled, "Разрешить расширение браузера")
                    .on_hover_text(
                        "Автозаполнение и сохранение логинов в браузере через локальный мост (docs/browser.md)",
                    );
                if enabled != self.browser_enabled {
                    self.browser_enabled = enabled;
                    save_settings(&self.settings_snapshot());
                    self.status = if enabled {
                        "Мост к браузеру включён.".to_string()
                    } else {
                        "Мост к браузеру выключен.".to_string()
                    };
                }
                ui.label(
                    egui::RichText::new(
                        "Расширение спрашивает у «Хранителя» записи для каждого сайта: первый раз — с вашим разрешением. Пароли не хранятся в браузере.",
                    )
                    .weak()
                    .small(),
                );

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                if ui
                    .checkbox(
                        &mut self.hide_to_tray,
                        "Сворачивать в трей при закрытии окна",
                    )
                    .on_hover_text("Крестик прячет окно, а не закрывает программу: горячие клавиши и мост продолжают работать")
                    .changed()
                {
                    save_settings(&self.settings_snapshot());
                }
                let mut autostart = self.autostart;
                if ui
                    .checkbox(
                        &mut autostart,
                        "Запускать при входе в Windows (свёрнутым в трей)",
                    )
                    .changed()
                {
                    match set_autostart(autostart) {
                        Ok(()) => {
                            self.autostart = autostart;
                            save_settings(&self.settings_snapshot());
                            self.status = if autostart {
                                "Автозапуск включён: при входе в Windows «Хранитель» запустится в трее."
                                    .to_string()
                            } else {
                                "Автозапуск выключен.".to_string()
                            };
                        }
                        Err(err) => {
                            self.hotkey_error = Some(format!("Автозапуск: {err}"));
                        }
                    }
                }
                ui.label(
                    egui::RichText::new(
                        "Без запущенного «Хранителя» не работают ни горячие клавиши, ни автозаполнение в браузере. Хранилище остаётся заблокированным, пока вы не введёте пароль или не подтвердите Windows Hello.",
                    )
                    .weak()
                    .small(),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() {
                        save = true;
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                });
            });

        // Пока идёт запись, следующая нажатая клавиша становится сочетанием.
        if self.hotkey_capture {
            let pressed = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, modifiers)) = pressed {
                if key == egui::Key::Escape {
                    self.hotkey_capture = false;
                } else if let Some(hotkey) = autotype::Hotkey::from_egui(key, modifiers) {
                    self.hotkey_capture = false;
                    self.hotkey_draft = hotkey.label();
                    self.apply_hotkey(hotkey);
                } else {
                    self.hotkey_error = Some(
                        "нужны модификатор (Ctrl, Alt или Shift) и буква, цифра либо F1–F24"
                            .to_string(),
                    );
                }
            }
            ctx.request_repaint();
        }

        // То же для сочетания быстрого доступа.
        if self.quick_capture {
            let pressed = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, modifiers)) = pressed {
                if key == egui::Key::Escape {
                    self.quick_capture = false;
                } else if let Some(hotkey) = autotype::Hotkey::from_egui(key, modifiers) {
                    self.quick_capture = false;
                    self.quick_draft = hotkey.label();
                    self.apply_quick_hotkey(hotkey);
                } else {
                    self.quick_error = Some(
                        "нужны модификатор (Ctrl, Alt или Shift) и буква, цифра либо F1–F24"
                            .to_string(),
                    );
                }
            }
            ctx.request_repaint();
        }

        if cancel || !open {
            self.settings_open = false;
            self.hotkey_capture = false;
            self.hotkey_error = None;
            self.quick_capture = false;
            self.quick_error = None;
            return;
        }

        if save {
            match autotype::Hotkey::parse(&self.hotkey_draft) {
                Err(err) => self.hotkey_error = Some(err),
                Ok(hotkey) => self.apply_hotkey(hotkey),
            }
            match autotype::Hotkey::parse(&self.quick_draft) {
                Err(err) => self.quick_error = Some(err),
                Ok(hotkey) => self.apply_quick_hotkey(hotkey),
            }
        }
    }

    /// Ставит новое сочетание. Если оно занято, остаётся старое,
    /// а ошибка показывается в диалоге — молча ничего не теряем.
    pub(crate) fn apply_hotkey(&mut self, hotkey: autotype::Hotkey) {
        match autotype::rebind(hotkey) {
            Ok(()) => {
                self.hotkey = hotkey;
                self.hotkey_draft = hotkey.label();
                self.hotkey_error = None;
                self.settings_open = false;
                save_settings(&self.settings_snapshot());
                self.status = format!("Горячая клавиша автоввода: {}", hotkey.label());
            }
            Err(err) => self.hotkey_error = Some(err),
        }
    }

    /// То же для сочетания быстрого доступа.
    pub(crate) fn apply_quick_hotkey(&mut self, hotkey: autotype::Hotkey) {
        match autotype::rebind_quick(hotkey) {
            Ok(()) => {
                self.quick_hotkey = hotkey;
                self.quick_draft = hotkey.label();
                self.quick_error = None;
                save_settings(&self.settings_snapshot());
                self.status = format!("Клавиша быстрого доступа: {}", hotkey.label());
            }
            Err(err) => self.quick_error = Some(err),
        }
    }
}

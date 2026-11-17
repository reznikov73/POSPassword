//! Окно генератора паролей.

use crate::*;

impl App {
    /// Диалог генератора паролей: случайные символы или фраза из слов.
    pub(crate) fn generator_dialog(&mut self, ctx: &egui::Context) {
        if !self.gen_open {
            return;
        }

        let mut open = true;
        let mut generate = false;
        let opts_before = self.gen_opts;
        let phrase_before = self.gen_phrase;
        let mode_before = self.gen_mode;

        egui::Window::new("Генератор паролей")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.gen_mode, GenMode::Chars, "Случайные символы");
                    ui.selectable_value(&mut self.gen_mode, GenMode::Phrase, "Запоминаемая фраза");
                });
                ui.separator();

                let bits = match self.gen_mode {
                    GenMode::Chars => {
                        ui.horizontal(|ui| {
                            ui.label("Длина");
                            ui.add(
                                egui::Slider::new(&mut self.gen_opts.length, 8..=64)
                                    .text("символов"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.gen_opts.lowercase, "строчные");
                            ui.checkbox(&mut self.gen_opts.uppercase, "заглавные");
                            ui.checkbox(&mut self.gen_opts.digits, "цифры");
                            ui.checkbox(&mut self.gen_opts.symbols, "символы");
                        });
                        ui.checkbox(
                            &mut self.gen_opts.avoid_ambiguous,
                            "без похожих символов (Il1O0)",
                        );
                        self.gen_opts.entropy_bits()
                    }
                    GenMode::Phrase => {
                        ui.horizontal(|ui| {
                            ui.label("Язык слов");
                            egui::ComboBox::from_id_salt("gen_lang")
                                .selected_text(self.gen_phrase.language.label())
                                .show_ui(ui, |ui| {
                                    for lang in Wordlist::all() {
                                        ui.selectable_value(
                                            &mut self.gen_phrase.language,
                                            lang,
                                            lang.label(),
                                        );
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Слов во фразе");
                            ui.add(
                                egui::Slider::new(&mut self.gen_phrase.words, 3..=8)
                                    .text("шт."),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Цифр в конце");
                            ui.add(
                                egui::Slider::new(&mut self.gen_phrase.digits, 0..=4)
                                    .text("шт."),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Разделитель");
                            egui::ComboBox::from_id_salt("gen_sep")
                                .selected_text(self.gen_phrase.separator.label())
                                .show_ui(ui, |ui| {
                                    for sep in Separator::all() {
                                        ui.selectable_value(
                                            &mut self.gen_phrase.separator,
                                            sep,
                                            sep.label(),
                                        );
                                    }
                                });
                            ui.checkbox(&mut self.gen_phrase.capitalize, "с заглавной буквы");
                        });
                        passphrase_entropy_bits(&self.gen_phrase)
                    }
                };

                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.gen_result)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "≈ {bits:.0} бит · {}",
                        strength_word(bits)
                    ))
                    .small()
                    .color(strength_color(bits)),
                );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Сгенерировать").clicked() {
                        generate = true;
                    }
                    if ui
                        .button("Копировать")
                        .on_hover_text("Буфер очистится через 30 секунд")
                        .clicked()
                    {
                        let result = self.gen_result.clone();
                        self.copy(&result, "Пароль");
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Фразу из слов проще запомнить и набрать, чем случайные символы, а стойкость при достаточном числе слов та же.",
                    )
                    .weak()
                    .small(),
                );
            });

        if !open {
            self.gen_open = false;
            self.gen_result.zeroize();
            return;
        }
        // Настройки поменяли — сразу показываем свежий образец. Смену языка
        // отслеживаем по отметке, а не по виду строки: иначе образец
        // пересоздавался на каждом кадре и мелькал без остановки.
        let language_changed = phrase_sample_stale(
            self.gen_mode,
            self.gen_sample_lang,
            self.gen_phrase.language,
        );
        if self.gen_opts != opts_before
            || self.gen_phrase != phrase_before
            || self.gen_mode != mode_before
            || language_changed
            || generate
        {
            self.regenerate();
        }
    }

    /// Генерирует образец по текущим настройкам генератора.
    pub(crate) fn regenerate(&mut self) {
        let result = match self.gen_mode {
            GenMode::Chars => generate_password(&self.gen_opts),
            GenMode::Phrase => generate_passphrase(&self.gen_phrase),
        };
        match result {
            Ok(password) => {
                self.gen_sample_lang = match self.gen_mode {
                    GenMode::Phrase => Some(self.gen_phrase.language),
                    GenMode::Chars => None,
                };
                self.gen_result = password;
            }
            Err(err) => self.status = format!("Не удалось сгенерировать: {err}"),
        }
    }
}

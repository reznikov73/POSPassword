//! Диалог создания и правки записи.

use eframe::egui;
use vault_core::{
    Field, GeneratorOptions, ItemKind, PassphraseOptions, generate_passphrase, generate_password,
    password_entropy_bits,
};

use crate::*;

pub(crate) fn draw_draft(
    ctx: &egui::Context,
    draft: &mut Option<Draft>,
    gen_opts: &GeneratorOptions,
    gen_phrase: &PassphraseOptions,
) -> DraftOutcome {
    let Some(d) = draft.as_mut() else {
        return DraftOutcome::Idle;
    };

    let mut outcome = DraftOutcome::Idle;
    let title = if d.id.is_some() {
        "Изменить запись"
    } else {
        "Новая запись"
    };

    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .default_width(540.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Тип:");
                let mut chosen = d.kind;
                egui::ComboBox::from_id_salt("draft_kind")
                    .selected_text(d.kind.label())
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for kind in ItemKind::all() {
                            ui.selectable_value(&mut chosen, kind, kind.label());
                        }
                    });
                if chosen != d.kind {
                    d.kind = chosen;
                    // Меняем заготовку полей, только если пользователь ничего не ввёл.
                    if d.fields.iter().all(|f| f.value.trim().is_empty()) {
                        d.fields = template_fields(chosen);
                    }
                }
            });
            ui.add_space(6.0);

            egui::Grid::new("draft_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Название");
                    ui.add(egui::TextEdit::singleline(&mut d.title).desired_width(380.0));
                    ui.end_row();

                    if d.kind == ItemKind::Login {
                        ui.label("Логин");
                        ui.add(egui::TextEdit::singleline(&mut d.username).desired_width(380.0));
                        ui.end_row();

                        ui.label("Пароль");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut d.password)
                                    .desired_width(210.0)
                                    .password(!d.show_plain),
                            );
                            if ui
                                .small_button(if d.show_plain { "Скрыть" } else { "Показать" })
                                .clicked()
                            {
                                d.show_plain = !d.show_plain;
                            }
                            if ui
                                .small_button("Случайный")
                                .on_hover_text("По настройкам из окна «Генератор»")
                                .clicked()
                            {
                                d.password = generate_password(gen_opts).unwrap_or_default();
                                d.show_plain = true;
                            }
                            if ui
                                .small_button("Фраза")
                                .on_hover_text(
                                    "Запоминаемая фраза, например «Корабль-Мандарин-Вишня-Сосна-42»",
                                )
                                .clicked()
                            {
                                d.password = generate_passphrase(gen_phrase).unwrap_or_default();
                                d.show_plain = true;
                            }
                            if ui
                                .small_button("Вставить")
                                .on_hover_text("Взять пароль из буфера обмена")
                                .clicked()
                                && let Ok(mut cb) = arboard::Clipboard::new()
                                && let Ok(text) = cb.get_text()
                            {
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    d.password = text;
                                    d.show_plain = true;
                                }
                            }
                        });
                        ui.end_row();

                        ui.label("");
                        ui.horizontal(|ui| {
                            let bits = password_entropy_bits(&d.password);
                            if bits > 0.0 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "≈ {bits:.0} бит · {}",
                                        strength_word(bits)
                                    ))
                                    .small()
                                    .color(strength_color(bits)),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(
                                        "придумайте пароль или нажмите «Случайный»/«Фраза»",
                                    )
                                    .weak()
                                    .small(),
                                );
                            }
                        });
                        ui.end_row();

                        ui.label("Сайт");
                        ui.add(egui::TextEdit::singleline(&mut d.url).desired_width(380.0));
                        ui.end_row();

                        ui.label("Ещё сайты");
                        ui.add(
                            egui::TextEdit::multiline(&mut d.urls)
                                .desired_rows(2)
                                .desired_width(380.0)
                                .hint_text("по одному адресу в строке: сайт, кабинет, форум"),
                        );
                        ui.end_row();
                    }
                });

            if !matches!(d.kind, ItemKind::Login | ItemKind::SecureNote) {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Поля").strong());
                let mut remove: Option<usize> = None;
                for (idx, field) in d.fields.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut field.label).desired_width(130.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut field.value)
                                .desired_width(220.0)
                                .password(field.sensitive && !d.show_plain),
                        );
                        if field.sensitive
                            && ui
                                .small_button(if d.show_plain {
                                    "Скрыть"
                                } else {
                                    "Показать"
                                })
                                .clicked()
                        {
                            d.show_plain = !d.show_plain;
                        }
                        if ui.small_button("✕").on_hover_text("Удалить поле").clicked()
                        {
                            remove = Some(idx);
                        }
                    });
                }
                if let Some(idx) = remove {
                    d.fields.remove(idx);
                }
                if ui.small_button("+ Добавить поле").clicked() {
                    d.fields.push(Field::new("Поле", ""));
                }
            }

            ui.add_space(6.0);
            ui.label("Заметки");
            ui.add(
                egui::TextEdit::multiline(&mut d.notes)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let can_save = !d.title.trim().is_empty();
                if ui
                    .add_enabled(can_save, egui::Button::new("Сохранить"))
                    .clicked()
                {
                    outcome = DraftOutcome::Save {
                        item: Box::new(d.build()),
                        is_new: d.id.is_none(),
                    };
                }
                if ui.button("Отмена").clicked() {
                    outcome = DraftOutcome::Cancel;
                }
                if !can_save {
                    ui.label(egui::RichText::new("Укажите название").weak().small());
                }
            });
        });

    outcome
}

//! Массовое добавление осмысленных описаний.

use crate::*;

impl App {
    /// Массовое добавление осмысленных описаний записям без заметок.
    pub(crate) fn descriptions_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if !self.desc_open {
            return;
        }

        let missing: Vec<&Item> = vault
            .items()
            .iter()
            .filter(|item| item.notes.trim().is_empty())
            .collect();
        let mut open = true;
        let mut apply = false;

        egui::Window::new("Осмысленные описания")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    "Программа собирает описание из названия, логина, сайта и открытых полей записи. Пароли и скрытые значения в описание не попадают.",
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Записей без описания: {} из {}.",
                        missing.len(),
                        vault.items().len()
                    ))
                    .strong(),
                );

                if missing.is_empty() {
                    ui.add_space(4.0);
                    ui.label("У всех записей уже есть заметки.");
                } else {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("desc_preview")
                        .auto_shrink([false, false])
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for item in missing.iter().take(5) {
                                ui.label(egui::RichText::new(&item.title).strong());
                                ui.label(
                                    egui::RichText::new(auto_description(item))
                                        .small()
                                        .weak(),
                                );
                                ui.add_space(4.0);
                            }
                            if missing.len() > 5 {
                                ui.label(
                                    egui::RichText::new(format!("…и ещё {}", missing.len() - 5))
                                        .weak()
                                        .small(),
                                );
                            }
                        });
                    ui.add_space(6.0);
                    if ui
                        .button(format!("Добавить описания ({})", missing.len()))
                        .clicked()
                    {
                        apply = true;
                    }
                    ui.label(
                        egui::RichText::new(
                            "Описание попадает в заметки записи — видно в списке, можно поправить кнопкой «Изменить».",
                        )
                        .weak()
                        .small(),
                    );
                }
            });

        if apply {
            match vault.describe_missing_notes() {
                Ok(count) => {
                    self.status =
                        format!("Добавлено описаний: {count}. Проверьте их кнопкой «Изменить».");
                    self.desc_open = false;
                }
                Err(err) => self.status = format!("Не удалось добавить описания: {err}"),
            }
        }
        if !open {
            self.desc_open = false;
        }
    }
}

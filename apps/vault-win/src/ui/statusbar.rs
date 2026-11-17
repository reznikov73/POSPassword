//! Строка состояния внизу окна.

use crate::*;

impl App {
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal(|ui| {
            if let Some(vault) = &self.vault {
                ui.label(
                    egui::RichText::new(format!("Записей: {}", vault.items().len()))
                        .small()
                        .weak(),
                );
                ui.separator();
            }
            ui.label(egui::RichText::new(&self.status).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut hint = if !autotype::is_ready() {
                    "автоввод недоступен".to_string()
                } else if let Some(title) = self.autotype_title() {
                    format!("Автоввод: {title}")
                } else {
                    format!("{} — автоввод", self.hotkey.label())
                };
                if autotype::is_quick_ready() {
                    hint.push_str(&format!(
                        " · {} — быстрый доступ",
                        self.quick_hotkey.label()
                    ));
                } else {
                    hint.push_str(" · быстрый доступ недоступен");
                }
                ui.label(egui::RichText::new(hint).small().weak());
                if self.clipboard.is_some() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("буфер обмена будет очищен автоматически")
                            .small()
                            .weak(),
                    );
                }
            });
        });
    }
}

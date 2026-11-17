//! Панель аудита хранилища.

use crate::*;

impl App {
    /// Панель аудита: сроки действия, повторяющиеся и короткие пароли.
    pub(crate) fn audit_dialog(&mut self, ctx: &egui::Context, vault: &Vault) {
        if !self.audit_open {
            return;
        }

        let report = vault.audit();
        let mut open = true;
        // Выбор записи и признак открытия занимаются по отдельности:
        // оба живут в замыкании окна.
        let selected = self.screen.selected_mut();
        let audit_open = &mut self.audit_open;

        egui::Window::new("Аудит хранилища")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Проверка идёт по расшифрованным записям на этом компьютере.",
                    )
                    .weak()
                    .small(),
                );
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("audit_scroll")
                    .auto_shrink([false, false])
                    .max_height(420.0)
                    .show(ui, |ui| {
                        if report.is_empty() {
                            ui.label(
                                "Нарушений не найдено: пароли длинные и разные, сроки в порядке.",
                            );
                            return;
                        }
                        if let Some(selected) = selected {
                            draw_findings(
                                ui,
                                "Сроки действия",
                                &report.expiring,
                                selected,
                                audit_open,
                            );
                            draw_findings(
                                ui,
                                "Повторяющиеся пароли",
                                &report.reused,
                                selected,
                                audit_open,
                            );
                            draw_findings(
                                ui,
                                "Короткие пароли",
                                &report.weak,
                                selected,
                                audit_open,
                            );
                            draw_findings(
                                ui,
                                "Давно не обновлялись",
                                &report.old,
                                selected,
                                audit_open,
                            );
                        }
                    });
            });

        if !open {
            self.audit_open = false;
        }
    }
}

/// Рисует группу находок аудита; клик выбирает запись и закрывает панель.
pub(crate) fn draw_findings(
    ui: &mut egui::Ui,
    title: &str,
    findings: &[vault_core::Finding],
    selected: &mut Option<Uuid>,
    open: &mut bool,
) {
    if findings.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!("{title} — {}", findings.len()))
            .strong()
            .size(15.0),
    );
    for finding in findings {
        let text = format!(
            "{} · {} — {}",
            finding.title,
            finding.kind.label(),
            finding.detail
        );
        if ui.selectable_label(false, text).clicked() {
            *selected = Some(finding.id);
            *open = false;
        }
    }
}

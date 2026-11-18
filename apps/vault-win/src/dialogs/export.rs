//! Диалог экспорта.

use crate::*;

impl App {
    /// Диалог экспорта копии хранилища.
    pub(crate) fn export_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if !self.export_open {
            return;
        }

        let mut open = true;
        let mut run = false;

        egui::Window::new("Экспорт хранилища")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(600.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(190, 130, 40),
                    "Файл будет содержать все пароли в открытом виде. Удалите его после переноса.",
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Формат:");
                    let mut chosen = self.export_format;
                    egui::ComboBox::from_id_salt("export_format")
                        .selected_text(self.export_format.label())
                        .show_ui(ui, |ui| {
                            for format in ExportFormat::all() {
                                ui.selectable_value(&mut chosen, format, format.label());
                            }
                        });
                    if chosen != self.export_format {
                        self.export_format = chosen;
                        if !self.export_path.trim().is_empty() {
                            self.export_path = PathBuf::from(self.export_path.trim())
                                .with_extension(chosen.extension())
                                .display()
                                .to_string();
                        }
                    }
                });

                ui.add_space(6.0);
                ui.label("Файл");
                ui.add(
                    egui::TextEdit::singleline(&mut self.export_path)
                        .hint_text("C:\\Users\\...\\hranitel-export.json")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                if ui.button("Куда сохранить…").clicked() {
                    let suggested = if self.export_path.trim().is_empty() {
                        default_export_path(vault.path(), self.export_format)
                    } else {
                        self.export_path.trim().to_string()
                    };
                    if let Some(path) = filedialog::save_file(&suggested) {
                        self.export_path = path.display().to_string();
                    }
                }
                if !self.export_path.trim().is_empty()
                    && Path::new(self.export_path.trim()).exists()
                {
                    ui.colored_label(
                        egui::Color32::from_rgb(190, 130, 40),
                        "Файл с таким именем уже есть — он будет перезаписан.",
                    );
                }
                ui.add_space(6.0);
                ui.label("Подтвердите мастер-пароль");
                ui.add(
                    egui::TextEdit::singleline(&mut self.export_password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );

                if let Some(result) = &self.export_result {
                    ui.add_space(8.0);
                    ui.label(result);
                }

                ui.add_space(10.0);
                if ui.button("Экспортировать").clicked() {
                    run = true;
                }
            });

        if !open {
            self.export_open = false;
            self.export_password.zeroize();
            return;
        }

        if run {
            let path = self.export_path.trim().to_string();
            if path.is_empty() {
                self.export_result = Some("Укажите путь к файлу.".to_string());
                return;
            }
            // Пароль сверяется с ключом уже открытого хранилища: повторное
            // `Vault::open` упёрлось бы в нашу же блокировку файла (Ф-6),
            // и верный пароль выглядел бы как неверный.
            if !export_password_ok(vault, &self.export_password) {
                self.export_result = Some("Мастер-пароль неверен.".to_string());
                return;
            }
            match vault.export(self.export_format) {
                Ok(text) => match std::fs::write(&path, text) {
                    Ok(()) => {
                        let count = vault.items().len();
                        self.status = format!("Экспортировано записей: {count}.");
                        self.export_result = Some(format!(
                            "Сохранено записей: {count}. Файл содержит открытые пароли — удалите его после переноса."
                        ));
                        self.export_password.zeroize();
                    }
                    Err(err) => {
                        self.export_result = Some(format!("Не удалось записать файл: {err}"));
                    }
                },
                Err(err) => self.export_result = Some(format!("Ошибка экспорта: {err}")),
            }
        }
    }
}

/// Проверяет мастер-пароль открытого хранилища перед экспортом (Ф-5).
///
/// Открывать файл повторно нельзя: он уже под нашей эксклюзивной
/// блокировкой (Ф-6), и `Vault::open` вернёт «занято» даже на верный
/// пароль. Поэтому сверяем выведенный ключ напрямую — тот же приём, что
/// в диалоге смены мастер-пароля.
pub(crate) fn export_password_ok(vault: &Vault, password: &str) -> bool {
    vault.verify_password(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_password_is_checked_against_open_vault() {
        let dir = tempfile::tempdir().expect("каталог");
        let path = dir.path().join("v.opvault");
        let vault = Vault::create(&path, "master-password-1").expect("создание");

        assert!(export_password_ok(&vault, "master-password-1"));
        assert!(!export_password_ok(&vault, "wrong-password-1"));
    }

    /// Регрессия Ф-5/Ф-6: повторное открытие своего же файла всегда «занято»,
    /// поэтому экспорт обязан проверять пароль через `verify_password`.
    #[test]
    fn reopening_own_vault_is_busy_but_verify_works() {
        let dir = tempfile::tempdir().expect("каталог");
        let path = dir.path().join("v.opvault");
        let vault = Vault::create(&path, "master-password-1").expect("создание");

        assert!(
            Vault::open(&path, "master-password-1").is_err(),
            "файл под нашей блокировкой — повторное открытие должно падать"
        );
        assert!(
            export_password_ok(&vault, "master-password-1"),
            "экспорт должен принимать верный пароль открытого хранилища"
        );
    }
}

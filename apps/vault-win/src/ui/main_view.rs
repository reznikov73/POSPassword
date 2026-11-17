//! Основной экран: тулбар, категории, список записей, карточка.

use crate::dialogs::draw_draft;
use crate::*;

impl App {
    pub(crate) fn main_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut vault) = self.screen.take_vault() else {
            return;
        };
        self.poll_hello_enable(&mut vault);
        let mut lock_requested = false;

        // Ctrl+C копирует пароль выбранной записи, но только когда ни одно
        // поле ввода не в фокусе — иначе сломается обычное копирование текста.
        // В палитре быстрого доступа Ctrl+C обслуживается самой палитрой.
        let no_text_focus = ui.ctx().memory(|memory| memory.focused().is_none());
        if no_text_focus
            && self.quick_access.is_none()
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::C))
            && let Some(id) = self.screen.selected()
            && let Some(item) = vault.get(id)
        {
            let password = item.password.clone();
            if !password.is_empty() {
                self.copy(&password, "Пароль");
            }
        }

        ui.horizontal(|ui| {
            ui.heading(APP_TITLE);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(vault.path().display().to_string())
                    .weak()
                    .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Заблокировать").clicked() {
                    lock_requested = true;
                }
            });
        });
        // Действия — второй строкой с переносом: даже в узком окне кнопки
        // не наезжают на путь к хранилищу.
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Быстрая вставка")
                .on_hover_text("Разобрать скопированный логин и пароль и создать запись")
                .clicked()
            {
                self.quick_open = true;
            }
            if ui
                .button("Генератор")
                .on_hover_text("Случайный пароль или запоминаемая фраза из слов")
                .clicked()
            {
                self.gen_open = true;
                if self.gen_result.is_empty() {
                    self.regenerate();
                }
            }
            if ui
                .button("Описания")
                .on_hover_text("Осмысленные описания для записей без заметок")
                .clicked()
            {
                self.desc_open = true;
            }
            if ui
                .button("Аудит")
                .on_hover_text("Слабые и повторяющиеся пароли, сроки действия")
                .clicked()
            {
                self.audit_open = true;
            }
            if ui
                .button("Импорт")
                .on_hover_text("Импорт из Bitwarden или 1Password")
                .clicked()
            {
                self.import_open = true;
                self.import_result = None;
            }
            if ui
                .button("Экспорт")
                .on_hover_text("Копия в JSON или CSV")
                .clicked()
            {
                self.export_open = true;
                self.export_result = None;
                self.export_password.clear();
                if self.export_path.is_empty() {
                    self.export_path = default_export_path(vault.path(), self.export_format);
                }
            }
            if ui
                .button("Пароль")
                .on_hover_text("Сменить мастер-пароль")
                .clicked()
            {
                self.password_open = true;
                self.password_error = None;
                self.password_current.clear();
                self.password_new.clear();
                self.password_confirm.clear();
            }
            let hello_on = sidecar_path(vault.path()).exists();
            let hello_label = if hello_on {
                "Hello: вкл"
            } else {
                "Hello: выкл"
            };
            if ui
                .button(hello_label)
                .on_hover_text("Разблокировка без мастер-пароля через Windows Hello")
                .clicked()
            {
                if hello_on {
                    match vault.disable_hello() {
                        Ok(()) => {
                            self.status = "Вход через Windows Hello выключен.".to_string();
                        }
                        Err(err) => {
                            self.status = format!("Не удалось выключить Hello: {err}");
                        }
                    }
                } else if self.hello_check.is_none() {
                    // Включение — только после проверки доступности Hello
                    // в фоновом потоке (Б-7): на машине без Hello кнопка
                    // сообщает причину и не создаёт сайдкар.
                    self.start_hello_check();
                }
            }
            if ui.button(format!("Тема: {}", self.theme.label())).clicked() {
                self.theme = self.theme.toggled();
                self.theme.apply(ui.ctx());
                save_settings(&self.settings_snapshot());
            }
            if ui.button("Справка").clicked() {
                self.help_open = true;
            }
            if ui.button("Настройки").clicked() {
                self.settings_open = true;
                self.hotkey_draft = self.hotkey.label();
                self.hotkey_error = None;
                self.hotkey_capture = false;
                self.hotkey_focus = true;
            }
        });
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(340.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let mut empty_query = String::new();
                    let query_target = self
                        .screen
                        .query_mut()
                        .unwrap_or(&mut empty_query);
                    ui.horizontal(|ui| {
                        let search = ui.add(
                            egui::TextEdit::singleline(query_target)
                                .hint_text("Поиск (Ctrl+F)")
                                .desired_width(250.0),
                        );
                        if self.focus_search {
                            search.request_focus();
                            self.focus_search = false;
                        }
                        if ui.button("+").on_hover_text("Новая запись").clicked() {
                            self.draft = Some(Draft::new_item());
                        }
                    });
                    ui.add_space(6.0);

                    // Категории: как вкладки над списком, с количеством записей.
                    let totals: Vec<(KindGroup, usize)> = KindGroup::all()
                        .iter()
                        .map(|group| {
                            let count = vault
                                .items()
                                .iter()
                                .filter(|item| group.matches(item.kind))
                                .count();
                            (*group, count)
                        })
                        .collect();
                    ui.horizontal_wrapped(|ui| {
                        for (group, count) in totals {
                            let label = format!("{} {count}", group.label());
                            if ui
                                .selectable_label(
                                    self.screen.kind_filter() == group,
                                    egui::RichText::new(label).small(),
                                )
                                .clicked()
                            {
                                self.screen.set_kind_filter(group);
                            }
                        }
                    });
                    ui.add_space(6.0);

                    let query_text = self
                        .screen
                        .query_mut()
                        .map(|query| query.clone())
                        .unwrap_or_default();
                    let kind_filter = self.screen.kind_filter();
                    let items: Vec<(Uuid, String, String, ItemKind, String)> = vault
                        .search(&query_text)
                        .into_iter()
                        .filter(|item| kind_filter.matches(item.kind))
                        .map(|i| {
                            (
                                i.id,
                                i.title.clone(),
                                i.username.clone(),
                                i.kind,
                                notes_snippet(&i.notes),
                            )
                        })
                        .collect();

                    egui::ScrollArea::vertical()
                        .id_salt("item_list")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if items.is_empty() {
                                ui.label(egui::RichText::new("Ничего не найдено").weak());
                            }
                            for (id, title, username, kind, snippet) in &items {
                                // Второй строкой — логин или тип записи, затем краткое
                                // описание из заметки, чтобы список сразу был говорящим.
                                let mut second = if !username.is_empty() {
                                    username.clone()
                                } else if !matches!(kind, ItemKind::Login | ItemKind::SecureNote)
                                {
                                    kind.label().to_string()
                                } else {
                                    String::new()
                                };
                                if !snippet.is_empty() {
                                    if !second.is_empty() {
                                        second.push_str(" · ");
                                    }
                                    second.push_str(snippet);
                                }
                                let label = if second.is_empty() {
                                    title.clone()
                                } else {
                                    format!("{title}\n{second}")
                                };
                                if ui
                                    .selectable_label(self.screen.selected() == Some(*id), label)
                                    .clicked()
                                {
                                    self.screen.set_selected(Some(*id));
                                    self.screen.set_reveal_password(false);
                                }
                            }
                        });
                },
            );

            ui.separator();

            ui.vertical(|ui| {
                let selected_item: Option<Item> = self
                        .screen
                        .selected()
                        .and_then(|id| vault.get(id))
                        .cloned();

                let Some(item) = selected_item else {
                    ui.add_space(18.0);
                    ui.label(
                        egui::RichText::new("Выберите запись слева или создайте новую (+).").weak(),
                    );
                    return;
                };

                egui::ScrollArea::vertical()
                    .id_salt("item_detail")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading(&item.title);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Удалить").clicked() {
                                        self.confirm_delete = Some(item.id);
                                    }
                                    if ui.button("Изменить").clicked() {
                                        self.draft = Some(Draft::from_item(&item));
                                    }
                                    if ui
                                        .button("Ввести в окно")
                                        .on_hover_text(
                                            "Через 3 секунды напечатать логин и пароль в активное окно",
                                        )
                                        .clicked()
                                    {
                                        // Печатает отдельный поток: отсчёт не зависит от кадров,
                                        // поэтому работает и когда окно свёрнуто.
                                        autotype::run_after(
                                            Duration::from_secs(3),
                                            ui.ctx().clone(),
                                        );
                                        self.autotype_countdown =
                                            Some(Instant::now() + Duration::from_secs(3));
                                    }
                                },
                            );
                        });
                        ui.label(egui::RichText::new(item.kind.label()).weak().small());
                        ui.add_space(10.0);

                        let expiry_days = expiries(&item, now_secs() as i64);
                        let totp_code = item
                            .fields
                            .iter()
                            .find(|field| {
                                field.label.to_ascii_lowercase().contains("totp")
                                    || field
                                        .value
                                        .trim()
                                        .to_ascii_lowercase()
                                        .starts_with("otpauth://")
                            })
                            .and_then(|field| Totp::parse(&field.value).ok())
                            .map(|totp| {
                                let now = now_secs();
                                (totp.code_at(now), totp.seconds_left(now))
                            });

                        egui::Grid::new("detail_grid")
                            .num_columns(2)
                            .spacing([12.0, 10.0])
                            .show(ui, |ui| {
                                if !item.username.is_empty() {
                                    ui.label("Логин");
                                    ui.horizontal(|ui| {
                                        ui.monospace(&item.username);
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(&item.username, "Логин");
                                        }
                                    });
                                    ui.end_row();
                                }

                                if !item.password.is_empty() {
                                    ui.label("Пароль");
                                    ui.horizontal(|ui| {
                                        let shown = if self.screen.reveal_password() {
                                            item.password.clone()
                                        } else {
                                            "•".repeat(item.password.chars().count().min(24))
                                        };
                                        ui.monospace(shown);
                                        let toggle = if self.screen.reveal_password() {
                                            "Скрыть"
                                        } else {
                                            "Показать"
                                        };
                                        if ui.small_button(toggle).clicked() {
                                            self.screen.set_reveal_password(!self.screen.reveal_password());
                                        }
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(&item.password, "Пароль");
                                        }
                                        let bits = password_entropy_bits(&item.password);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "≈ {bits:.0} бит · {}",
                                                strength_word(bits)
                                            ))
                                            .small()
                                            .color(strength_color(bits)),
                                        );
                                    });
                                    ui.end_row();
                                }

                                let item_urls = item.all_urls();
                                for (idx, url) in item_urls.iter().enumerate() {
                                    ui.label(if idx == 0 { "Сайт" } else { "Ещё сайт" });
                                    ui.horizontal(|ui| {
                                        ui.monospace(*url);
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(url, "Сайт");
                                        }
                                    });
                                    ui.end_row();
                                }

                                for field in &item.fields {
                                    if field.value.is_empty() {
                                        continue;
                                    }
                                    ui.label(&field.label);
                                    ui.horizontal(|ui| {
                                        let shown = if field.sensitive && !self.screen.reveal_password() {
                                            "•".repeat(field.value.chars().count().min(20))
                                        } else {
                                            field.value.clone()
                                        };
                                        ui.monospace(shown);
                                        if field.sensitive {
                                            let toggle = if self.screen.reveal_password() {
                                                "Скрыть"
                                            } else {
                                                "Показать"
                                            };
                                            if ui.small_button(toggle).clicked() {
                                                self.screen.set_reveal_password(!self.screen.reveal_password());
                                            }
                                        }
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(&field.value, &field.label);
                                        }
                                        if let Some(expiry) = expiry_days
                                            .iter()
                                            .find(|expiry| expiry.label == field.label)
                                        {
                                            let days = expiry.days_left;
                                            let (text, color) = if days < 0 {
                                                (
                                                    format!("просрочен на {} дн.", -days),
                                                    egui::Color32::from_rgb(200, 70, 70),
                                                )
                                            } else {
                                                (
                                                    format!("осталось {} дн.", days),
                                                    egui::Color32::from_rgb(190, 130, 40),
                                                )
                                            };
                                            ui.colored_label(
                                                color,
                                                egui::RichText::new(text).small(),
                                            );
                                        }
                                    });
                                    ui.end_row();
                                }

                                if let Some((code, seconds_left)) = &totp_code {
                                    ui.label("Код TOTP");
                                    ui.horizontal(|ui| {
                                        ui.monospace(code);
                                        ui.label(
                                            egui::RichText::new(format!("ещё {seconds_left} с"))
                                                .weak()
                                                .small(),
                                        );
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(code, "Код TOTP");
                                        }
                                    });
                                    ui.end_row();
                                }
                            });

                        ui.add_space(12.0);
                        ui.label("Заметки");
                        if item.notes.is_empty() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(
                                        "Пусто — можно составить осмысленное описание автоматически.",
                                    )
                                    .weak()
                                    .small(),
                                );
                                if ui
                                    .small_button("Описать")
                                    .on_hover_text("Собрать описание из данных записи")
                                    .clicked()
                                {
                                    self.describe_one = Some(item.id);
                                }
                            });
                        } else {
                            ui.label(&item.notes);
                        }
                    });
            });
        });

        // Кнопка «Описать» из карточки: описание пишется в заметки записи.
        if let Some(id) = self.describe_one.take()
            && let Some(item) = vault.get(id)
        {
            let mut updated = item.clone();
            updated.notes = auto_description(&updated);
            match vault.update(updated) {
                Ok(()) => {
                    self.status =
                        "Описание добавлено в заметки — поправьте его кнопкой «Изменить»."
                            .to_string();
                }
                Err(err) => self.status = format!("Не удалось добавить описание: {err}"),
            }
        }

        if let Some(id) = self.confirm_delete {
            let title = vault.get(id).map(|i| i.title.clone()).unwrap_or_default();
            let mut delete = false;
            let mut cancel = false;

            egui::Window::new("Удалить запись?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "«{title}» будет удалена без возможности восстановления."
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Удалить").clicked() {
                            delete = true;
                        }
                        if ui.button("Отмена").clicked() {
                            cancel = true;
                        }
                    });
                });

            if delete {
                match vault.remove(id) {
                    Ok(()) => {
                        self.status = "Запись удалена.".to_string();
                        if self.screen.selected() == Some(id) {
                            self.screen.set_selected(None);
                        }
                    }
                    Err(err) => self.status = format!("Не удалось удалить: {err}"),
                }
                self.confirm_delete = None;
            } else if cancel {
                self.confirm_delete = None;
            }
        }

        if self.import_open {
            let mut close = false;
            let mut run = false;

            egui::Window::new("Импорт из другого менеджера")
                .collapsible(false)
                .resizable(false)
                .default_width(560.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.label("Файл экспорта (.json или .csv)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.import_path)
                            .hint_text("C:\\Users\\...\\bitwarden_export.json")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Выбрать файл…").clicked()
                            && let Some(path) = filedialog::pick_file()
                        {
                            self.import_path = path.display().to_string();
                        }
                        if ui
                            .button("Вставить путь")
                            .on_hover_text(
                                "Взять файл, скопированный в Проводнике, или текст пути из буфера",
                            )
                            .clicked()
                        {
                            if let Some(path) = filedialog::clipboard_file() {
                                self.import_path = path.display().to_string();
                            } else if let Some(text) = filedialog::clipboard_text() {
                                self.import_path = text;
                            } else {
                                self.import_result =
                                    Some("В буфере нет ни файла, ни пути.".to_string());
                            }
                        }
                        ui.label(
                            egui::RichText::new("или перетащите файл в окно")
                                .weak()
                                .small(),
                        );
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Формат:");
                        egui::ComboBox::from_id_salt("import_format")
                            .selected_text(self.import_format.label())
                            .show_ui(ui, |ui| {
                                for format in ImportFormat::all() {
                                    ui.selectable_value(
                                        &mut self.import_format,
                                        format,
                                        format.label(),
                                    );
                                }
                            });
                    });

                    if let Some(result) = &self.import_result {
                        ui.add_space(8.0);
                        ui.label(result);
                    }

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Импортировать").clicked() {
                            run = true;
                        }
                        if ui.button("Закрыть").clicked() {
                            close = true;
                        }
                    });
                    ui.label(
                        egui::RichText::new("Совпадения по названию и логину пропускаются.")
                            .weak()
                            .small(),
                    );
                });

            if run {
                let path = self.import_path.trim().to_string();
                if path.is_empty() {
                    self.import_result = Some("Укажите путь к файлу.".to_string());
                } else {
                    match import_file(&mut vault, &path, self.import_format) {
                        Ok(summary) => {
                            let mut text = format!("Добавлено: {}.", summary.added);
                            if summary.duplicates > 0 {
                                text.push_str(&format!(
                                    " Пропущено дубликатов: {}.",
                                    summary.duplicates
                                ));
                            }
                            if summary.unsupported > 0 {
                                text.push_str(&format!(
                                    " Не поддержано записей: {}.",
                                    summary.unsupported
                                ));
                            }
                            self.status = text.clone();
                            self.import_result = Some(text);
                        }
                        Err(err) => {
                            self.import_result = Some(format!("Ошибка: {err}"));
                        }
                    }
                }
            }

            if close {
                self.import_open = false;
                self.import_result = None;
            }
        }

        match draw_draft(ui.ctx(), &mut self.draft, &self.gen_opts, &self.gen_phrase) {
            DraftOutcome::Idle => {}
            DraftOutcome::Cancel => self.draft = None,
            DraftOutcome::Save { item, is_new } => {
                let item = *item;
                let result = if is_new {
                    vault.add(item.clone()).map(|_| ())
                } else {
                    vault.update(item.clone())
                };
                match result {
                    Ok(()) => {
                        self.screen.set_selected(Some(item.id));
                        self.draft = None;
                        self.status = "Сохранено.".to_string();
                    }
                    Err(err) => self.status = format!("Ошибка сохранения: {err}"),
                }
            }
        }

        self.password_dialog(ui.ctx(), &mut vault);
        self.export_dialog(ui.ctx(), &mut vault);
        self.audit_dialog(ui.ctx(), &vault);
        self.quick_add_dialog(ui.ctx(), &mut vault);
        self.quick_access_dialog(ui.ctx(), &mut vault);
        self.descriptions_dialog(ui.ctx(), &mut vault);
        self.generator_dialog(ui.ctx());

        if lock_requested {
            self.lock("Заблокировано.");
        } else {
            self.screen.put_vault(vault);
        }
    }
}

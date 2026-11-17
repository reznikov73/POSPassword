//! Основной экран: тулбар, чипсы категорий, список записей, карточка.

use eframe::egui;

use crate::dialogs::draw_draft;
use crate::*;

impl App {
    pub(crate) fn main_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut vault) = self.screen.take_vault() else {
            return;
        };
        self.poll_hello_enable(&mut vault);

        // Ctrl+1..8 переключают категории (И-2). Проверка до отрисовки,
        // чтобы нажатие не добиралось до полей ввода.
        let digit_keys = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
        ];
        for (idx, group) in KindGroup::all().into_iter().enumerate() {
            if ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, digit_keys[idx]))
            {
                self.screen.set_kind_filter(group);
            }
        }

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

        let mut lock_requested = false;
        let mut backup_request = false;

        // Заголовок: название, путь к хранилищу и блокировка (макет A).
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
        ui.add_space(2.0);

        // Тулбар по макету A (И-1): слева действия, справа аудит и настройки.
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(egui::RichText::new("+ Запись").strong())
                .on_hover_text("Новая запись (Ctrl+N — кнопка «+» слева)")
                .clicked()
            {
                self.draft = Some(Draft::new_item());
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
                .button("Быстрая вставка")
                .on_hover_text("Разобрать скопированный логин и пароль и создать запись")
                .clicked()
            {
                self.quick_open = true;
            }
            if ui
                .button("Импорт…")
                .on_hover_text("Импорт из Bitwarden, 1Password или Яндекс Браузера")
                .clicked()
            {
                self.import_open = true;
                self.import_result = None;
            }
            if ui
                .button("Экспорт…")
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Справка F1")
                    .on_hover_text("Встроенная справка (или F1)")
                    .clicked()
                {
                    self.help_open = true;
                }
                egui::menu::MenuButton::new("⚙").ui(ui, |ui| {
                    if ui.button("Настройки…").clicked() {
                        self.settings_open = true;
                        self.hotkey_draft = self.hotkey.label();
                        self.hotkey_error = None;
                        self.hotkey_capture = false;
                        self.hotkey_focus = true;
                    }
                    if ui.button("Описания…").clicked() {
                        self.desc_open = true;
                    }
                    if ui.button("Сменить мастер-пароль…").clicked() {
                        self.password_open = true;
                        self.password_error = None;
                        self.password_current.clear();
                        self.password_new.clear();
                        self.password_confirm.clear();
                    }
                    if ui.button("Резервная копия…").clicked() {
                        backup_request = true;
                    }
                    if ui.button(format!("Тема: {}", self.theme.label())).clicked() {
                        self.theme = self.theme.toggled();
                        self.theme.apply(ui.ctx());
                        save_settings(&self.settings_snapshot());
                    }
                });

                let hello_on = sidecar_path(vault.path()).exists();
                let hello_label = if self.hello_check.is_some() {
                    "Hello: проверка…"
                } else if hello_on {
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

                // Аудит со счётчиком (И-1): янтарный при находках,
                // зелёная галочка при чистом хранилище.
                let findings = self.audit_total(&vault);
                let audit_text = if findings == 0 {
                    "Аудит ✓".to_string()
                } else {
                    format!("Аудит · {findings}")
                };
                let audit_button = if findings == 0 {
                    egui::Button::new(egui::RichText::new(audit_text).color(GREEN))
                } else {
                    egui::Button::new(
                        egui::RichText::new(audit_text)
                            .color(egui::Color32::from_rgb(122, 74, 8))
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(250, 238, 220))
                };
                if ui
                    .add(audit_button)
                    .on_hover_text("Слабые и повторяющиеся пароли, сроки действия")
                    .clicked()
                {
                    self.audit_open = true;
                }
            });
        });
        ui.separator();

        // Контент: список слева, карточка справа.
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(340.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let mut intents = ListIntents::default();
                    self.item_list(ui, &vault, &mut intents);
                    intents.apply(self, &mut vault);
                },
            );

            ui.separator();

            let mut card_intents = CardIntents::default();
            self.item_card(ui, &vault, &mut card_intents);
            card_intents.apply(self, &mut vault);
        });

        // Резервная копия в один клик (Ф-5): системный диалог сохранения.
        if backup_request {
            let default_path = vault
                .path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(format!("vault-{}.opvault", date_ymd(now_secs() as i64)));
            if let Some(target) = filedialog::save_vault_copy(&default_path.display().to_string()) {
                match std::fs::copy(vault.path(), &target) {
                    Ok(_) => {
                        self.status = format!("Резервная копия: {}", target.display());
                    }
                    Err(err) => {
                        self.status = format!("Не удалось сделать копию: {err}");
                    }
                }
            }
        }

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
                        "«{title}» будет удалена. 10 секунд её можно вернуть кнопкой «Вернуть»."
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
                match vault.get(id).cloned() {
                    Some(item) => match vault.remove(id) {
                        Ok(()) => {
                            self.status = "Запись удалена. Вернуть можно 10 секунд.".to_string();
                            if self.screen.selected() == Some(id) {
                                self.screen.set_selected(None);
                            }
                            // Отложенное восстановление (И-6): Drop записи
                            // зануляет секреты, если таймаут истёк.
                            self.undo = Some((item, Instant::now()));
                        }
                        Err(err) => self.status = format!("Не удалось удалить: {err}"),
                    },
                    None => self.confirm_delete = None,
                }
                self.confirm_delete = None;
            } else if cancel {
                self.confirm_delete = None;
            }
        }

        // Toast «Запись удалена · Вернуть» (И-6).
        let mut undo_expired = false;
        let mut undo_restore = false;
        if let Some((item, at)) = &self.undo {
            let left = UNDO_TTL.saturating_sub(at.elapsed());
            if left.is_zero() {
                undo_expired = true;
            } else {
                egui::Window::new("undo-toast")
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -10.0))
                    .show(ui.ctx(), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(format!("Запись «{}» удалена", item.title));
                            if ui
                                .button(format!("Вернуть · {} с", left.as_secs()))
                                .clicked()
                            {
                                undo_restore = true;
                            }
                        });
                    });
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        }
        if undo_expired {
            self.undo = None;
        }
        if undo_restore && let Some((item, _)) = self.undo.take() {
            match vault.restore(item.clone()) {
                Ok(()) => {
                    self.screen.set_selected(Some(item.id));
                    self.status = "Запись возвращена.".to_string();
                }
                Err(err) => self.status = format!("Не удалось вернуть запись: {err}"),
            }
        }

        // Напоминание об истекающих сроках (И-9): один раз за разблокировку.
        let mut reminder_open_audit = false;
        if let Some((count, at)) = self.reminder {
            if at.elapsed() < Duration::from_secs(20) {
                egui::Window::new("expiry-toast")
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -56.0))
                    .show(ui.ctx(), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Истекает или просрочено: {count}"))
                                    .color(AMBER),
                            );
                            if ui.button("Аудит").clicked() {
                                reminder_open_audit = true;
                            }
                        });
                    });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else {
                self.reminder = None;
            }
        }
        if reminder_open_audit {
            self.reminder = None;
            self.audit_open = true;
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

    /// Левая колонка: поиск, чипсы категорий и список записей (И-2, И-3).
    ///
    /// Действия контекстного меню собираются в `intents` и применяются
    /// после отрисовки — меню рисуется при заёме хранилища.
    fn item_list(&mut self, ui: &mut egui::Ui, vault: &Vault, intents: &mut ListIntents) {
        let mut empty_query = String::new();
        let query_target = self.screen.query_mut().unwrap_or(&mut empty_query);
        ui.horizontal(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(query_target)
                    .hint_text("Поиск (Ctrl+F)…")
                    .desired_width(f32::INFINITY),
            );
            if self.focus_search {
                search.request_focus();
                self.focus_search = false;
            }
        });
        ui.add_space(6.0);

        // Чипсы категорий со счётчиками (И-2): активный залит акцентом.
        ui.horizontal_wrapped(|ui| {
            for group in KindGroup::all() {
                let count = vault
                    .items()
                    .iter()
                    .filter(|item| group.matches_item(item))
                    .count();
                let label = format!("{} {count}", group.label());
                let active = self.screen.kind_filter() == group;
                let accent = accent_color(ui);
                let text = if active {
                    egui::RichText::new(label)
                        .small()
                        .strong()
                        .color(egui::Color32::WHITE)
                } else {
                    egui::RichText::new(label).small()
                };
                let button = egui::Button::new(text)
                    .small()
                    .corner_radius(egui::CornerRadius::same(12))
                    .fill(if active {
                        accent
                    } else {
                        ui.visuals().window_fill
                    });
                if ui
                    .add(button)
                    .on_hover_text(format!(
                        "Категория «{}» (Ctrl+{})",
                        group.label(),
                        KindGroup::all()
                            .iter()
                            .position(|g| *g == group)
                            .unwrap_or(0)
                            + 1
                    ))
                    .clicked()
                {
                    self.screen.set_kind_filter(group);
                }
            }
        });
        ui.add_space(6.0);

        let kind_filter = self.screen.kind_filter();
        let items: Vec<&Item> = {
            let query_text = self
                .screen
                .query_mut()
                .map(|query| query.clone())
                .unwrap_or_default();
            vault
                .search(&query_text)
                .into_iter()
                .filter(|item| kind_filter.matches_item(item))
                .collect()
        };
        let total = vault.items().len();

        egui::ScrollArea::vertical()
            .id_salt("item_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if total == 0 {
                    // Пустое хранилище (И-10): приглашение, а не ошибка.
                    ui.add_space(14.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Хранилище пусто").strong());
                        ui.label(
                            egui::RichText::new(
                                "Нажмите «+ Запись» вверху, чтобы сохранить первый пароль.",
                            )
                            .weak()
                            .small(),
                        );
                    });
                    return;
                }
                if items.is_empty() {
                    // Ничего не найдено (И-10).
                    ui.add_space(14.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Ничего не найдено").strong());
                        ui.label(
                            egui::RichText::new("Esc — очистить поиск, Ctrl+1..8 — категории")
                                .weak()
                                .small(),
                        );
                    });
                    return;
                }
                let now = now_secs() as i64;
                for item in items {
                    self.item_row(ui, item, now, intents);
                }
            });
    }

    /// Одна строка списка (И-3): аватар типа, название, подзаголовок,
    /// бейджи справа; выделение — рамка и мягкая заливка; ПКМ — меню.
    fn item_row(&mut self, ui: &mut egui::Ui, item: &Item, now: i64, intents: &mut ListIntents) {
        let selected = self.screen.selected() == Some(item.id);
        let accent = accent_color(ui);

        // Подзаголовок: логин и/или сайт, иначе тип записи.
        let mut subtitle = item.username.clone();
        if let Some(host) = item.all_urls().first().and_then(|url| normalize_host(url)) {
            let base = registrable_domain(&host);
            if !base.is_empty() {
                if !subtitle.is_empty() {
                    subtitle.push_str(" · ");
                }
                subtitle.push_str(&base);
            }
        }
        if subtitle.is_empty() && !matches!(item.kind, ItemKind::Login | ItemKind::SecureNote) {
            subtitle = item.kind.label().to_string();
        }
        if subtitle.is_empty() {
            let snippet = notes_snippet(&item.notes);
            if !snippet.is_empty() {
                subtitle = snippet;
            }
        }

        // Бейджи (И-3): TOTP, срок, звезда.
        let has_totp = item.fields.iter().any(|field| {
            field.label.to_ascii_lowercase().contains("totp")
                || field
                    .value
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("otpauth://")
        });
        let nearest_expiry = expiries(item, now)
            .into_iter()
            .min_by_key(|expiry| expiry.days_left);

        let frame = egui::Frame::default()
            .fill(if selected {
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 36)
            } else {
                egui::Color32::TRANSPARENT
            })
            .stroke(if selected {
                egui::Stroke::new(1.0, accent)
            } else {
                egui::Stroke::NONE
            })
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Аватар: круг с буквой типа.
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    let avatar_fill = egui::Color32::from_rgba_unmultiplied(
                        accent.r(),
                        accent.g(),
                        accent.b(),
                        46,
                    );
                    ui.painter().circle_filled(rect.center(), 12.0, avatar_fill);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        kind_letter(item.kind).to_string(),
                        egui::FontId::proportional(13.0),
                        ui.visuals().text_color(),
                    );

                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&item.title).strong()));
                        if !subtitle.is_empty() {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&subtitle).small().weak())
                                    .truncate(),
                            );
                        }
                    });

                    // Бейджи справа.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if item.favorite {
                            ui.label(egui::RichText::new("★").color(AMBER).small());
                        }
                        if let Some(expiry) = nearest_expiry {
                            let (text, color) = if expiry.days_left < 0 {
                                (format!("−{} дн", -expiry.days_left), RED)
                            } else if expiry.days_left <= 90 {
                                (format!("{} дн", expiry.days_left), AMBER)
                            } else {
                                (String::new(), egui::Color32::TRANSPARENT)
                            };
                            if !text.is_empty() {
                                ui.label(egui::RichText::new(text).color(color).small());
                            }
                        }
                        if has_totp {
                            ui.label(egui::RichText::new("TOTP").color(GREEN).small());
                        }
                    });
                });
            });
        let response = frame
            .response
            .interact(egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            self.screen.set_selected(Some(item.id));
            self.screen.set_reveal_password(false);
        }

        // Контекстное меню (И-3).
        response.context_menu(|ui| {
            if ui.button("Изменить").clicked() {
                intents.edit = Some(item.id);
            }
            if ui.button("Копировать логин").clicked() && !item.username.is_empty() {
                intents.copy_username = Some(item.username.clone());
            }
            if ui.button("Копировать пароль").clicked() && !item.password.is_empty()
            {
                intents.copy_password = Some(item.password.clone());
            }
            let star_label = if item.favorite {
                "Убрать из избранного"
            } else {
                "В избранное"
            };
            if ui.button(star_label).clicked() {
                intents.toggle_favorite = Some(item.id);
            }
            if ui.button("Удалить…").clicked() {
                intents.delete = Some(item.id);
            }
        });

        ui.add_space(1.0);
    }

    /// Правая колонка: карточка выбранной записи (И-4).
    fn item_card(&mut self, ui: &mut egui::Ui, vault: &Vault, intents: &mut CardIntents) {
        let selected_item: Option<Item> =
            self.screen.selected().and_then(|id| vault.get(id)).cloned();

        let Some(item) = selected_item else {
            ui.add_space(18.0);
            ui.label(
                egui::RichText::new("Выберите запись слева или создайте новую («+ Запись»).")
                    .weak(),
            );
            return;
        };

        egui::ScrollArea::vertical()
            .id_salt("item_detail")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Заголовок карточки: название, звезда, тип, действия.
                ui.horizontal(|ui| {
                    ui.heading(&item.title);
                    let star = if item.favorite { "★" } else { "☆" };
                    if ui
                        .button(egui::RichText::new(star).color(AMBER).size(18.0))
                        .on_hover_text("Избранное (фильтр «★ Избранное», Ctrl+8)")
                        .clicked()
                    {
                        intents.toggle_favorite = Some(item.id);
                    }
                    let chip = egui::Button::new(
                        egui::RichText::new(item.kind.label())
                            .small()
                            .color(accent_color(ui)),
                    )
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(egui::CornerRadius::same(10));
                    ui.add(chip);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(egui::RichText::new("Удалить").color(RED))
                            .clicked()
                        {
                            intents.delete = Some(item.id);
                        }
                        if ui.button("Изменить").clicked() {
                            intents.edit = Some(item.id);
                        }
                    });
                });

                // TOTP-блок (И-4): код крупно, кольцо отсчёта, кнопка «Код».
                // Секрет в карточке не показывается — он доступен только
                // в диалоге правки, для переноса на другое устройство.
                let totp_secret_field = item.fields.iter().find(|field| {
                    field.label.to_ascii_lowercase().contains("totp")
                        || field
                            .value
                            .trim()
                            .to_ascii_lowercase()
                            .starts_with("otpauth://")
                });
                if let Some(field) = totp_secret_field
                    && let Ok(totp) = Totp::parse(&field.value)
                {
                    let now = now_secs();
                    let code = totp.code_at(now);
                    let seconds_left = totp.seconds_left(now);
                    egui::Frame::group(ui.style())
                        .fill(ui.visuals().faint_bg_color)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new("Одноразовый код (TOTP)")
                                            .weak()
                                            .small(),
                                    );
                                    let spaced: String = if code.len() >= 6 {
                                        format!("{} {}", &code[..3], &code[3..])
                                    } else {
                                        code.clone()
                                    };
                                    ui.label(
                                        egui::RichText::new(spaced)
                                            .monospace()
                                            .size(24.0)
                                            .strong()
                                            .color(accent_color(ui)),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Код").clicked() {
                                            intents.copy_totp = Some(code);
                                        }
                                        totp_ring(ui, seconds_left as u32, 30);
                                    },
                                );
                            });
                        });
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                    ui.add_space(6.0);
                }

                let expiry_days = expiries(&item, now_secs() as i64);

                egui::Grid::new("detail_grid")
                    .num_columns(2)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        if !item.username.is_empty() {
                            ui.label("Логин");
                            ui.horizontal(|ui| {
                                ui.monospace(&item.username);
                                if ui.small_button("⧉").clicked() {
                                    intents.copy_username = Some(item.username.clone());
                                }
                            });
                            ui.end_row();
                        }

                        if !item.password.is_empty() {
                            ui.label("Пароль");
                            ui.vertical(|ui| {
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
                                        self.screen
                                            .set_reveal_password(!self.screen.reveal_password());
                                    }
                                    if ui.small_button("⧉").clicked() {
                                        intents.copy_password = Some(item.password.clone());
                                    }
                                });
                                // Шкала стойкости (И-4): 5 сегментов, биты, вердикт.
                                let bits = password_entropy_bits(&item.password);
                                ui.horizontal(|ui| {
                                    strength_scale(ui, bits);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "≈ {bits:.0} бит · {}",
                                            strength_word(bits)
                                        ))
                                        .small()
                                        .color(strength_color(bits)),
                                    );
                                });
                            });
                            ui.end_row();
                        }

                        // Сайты и теги — чипсами (И-4).
                        let item_urls = item.all_urls();
                        if !item_urls.is_empty() {
                            ui.label("Сайт");
                            ui.vertical(|ui| {
                                for url in item_urls {
                                    let chip = egui::Button::new(
                                        egui::RichText::new(url)
                                            .small()
                                            .color(ui.visuals().hyperlink_color),
                                    )
                                    .fill(ui.visuals().faint_bg_color)
                                    .corner_radius(egui::CornerRadius::same(8));
                                    if ui.add(chip).clicked() {
                                        intents.copy_url = Some(url.to_string());
                                    }
                                }
                            });
                            ui.end_row();
                        }
                        if !item.tags.is_empty() {
                            ui.label("Теги");
                            ui.horizontal_wrapped(|ui| {
                                for tag in &item.tags {
                                    let chip = egui::Button::new(
                                        egui::RichText::new(tag.as_str()).small(),
                                    )
                                    .fill(ui.visuals().faint_bg_color)
                                    .corner_radius(egui::CornerRadius::same(8));
                                    ui.add(chip);
                                }
                            });
                            ui.end_row();
                        }

                        for field in &item.fields {
                            if field.value.is_empty() {
                                continue;
                            }
                            // Поле с секретом TOTP уже показано блоком выше.
                            if Some(std::ptr::from_ref(field))
                                == totp_secret_field.map(std::ptr::from_ref)
                            {
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
                                        self.screen
                                            .set_reveal_password(!self.screen.reveal_password());
                                    }
                                }
                                if ui.small_button("⧉").clicked() {
                                    intents.copy_field =
                                        Some((field.label.clone(), field.value.clone()));
                                }
                                if let Some(expiry) = expiry_days
                                    .iter()
                                    .find(|expiry| expiry.label == field.label)
                                {
                                    let days = expiry.days_left;
                                    let (text, color) = if days < 0 {
                                        (format!("просрочен на {} дн.", -days), RED)
                                    } else {
                                        (format!("осталось {} дн.", days), AMBER)
                                    };
                                    ui.colored_label(color, egui::RichText::new(text).small());
                                }
                            });
                            ui.end_row();
                        }
                    });

                // История пароля (И-5): раскрываемый блок, восстановление.
                if !item.password_history.is_empty() {
                    ui.add_space(8.0);
                    let header = format!("История пароля: {} верс", item.password_history.len());
                    egui::CollapsingHeader::new(header)
                        .default_open(false)
                        .id_salt("password_history")
                        .show(ui, |ui| {
                            for (index, version) in item.password_history.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(date_ymd(version.changed_at))
                                            .weak()
                                            .small(),
                                    );
                                    ui.monospace("•".repeat(version.value.chars().count().min(14)));
                                    if ui.small_button("⧉").clicked() {
                                        intents.copy_field = Some((
                                            "Прежний пароль".to_string(),
                                            version.value.clone(),
                                        ));
                                    }
                                    if ui
                                        .small_button("Сделать текущим")
                                        .on_hover_text(
                                            "Вернуть этот пароль; нынешний уйдёт в историю",
                                        )
                                        .clicked()
                                    {
                                        intents.restore_version = Some((item.id, index));
                                    }
                                });
                            }
                        });
                }

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

                // Даты и автоввод (И-4): внизу карточки.
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Создано {} · Обновлено {}",
                        date_ymd(item.created_at),
                        date_ymd(item.updated_at)
                    ))
                    .weak()
                    .small(),
                );
                ui.add_space(8.0);
                let countdown = self
                    .autotype_countdown
                    .map(|deadline| {
                        (deadline.saturating_duration_since(Instant::now())).as_secs() + 1
                    })
                    .unwrap_or(3);
                if ui
                    .button(format!("Ввести в окно · {countdown} с"))
                    .on_hover_text("Через 3 секунды напечатать логин и пароль в активное окно")
                    .clicked()
                {
                    // Печатает отдельный поток: отсчёт не зависит от кадров,
                    // поэтому работает и когда окно свёрнуто.
                    autotype::run_after(Duration::from_secs(3), ui.ctx().clone());
                    self.autotype_countdown = Some(Instant::now() + Duration::from_secs(3));
                }
            });
    }
}

/// Действия из списка, применяемые после отрисовки (И-3).
#[derive(Default)]
struct ListIntents {
    edit: Option<Uuid>,
    copy_username: Option<String>,
    copy_password: Option<String>,
    toggle_favorite: Option<Uuid>,
    delete: Option<Uuid>,
}

impl ListIntents {
    fn apply(self, app: &mut App, vault: &mut Vault) {
        if let Some(id) = self.edit
            && let Some(item) = vault.get(id)
        {
            app.draft = Some(Draft::from_item(item));
        }
        if let Some(username) = self.copy_username {
            app.copy(&username, "Логин");
        }
        if let Some(password) = self.copy_password {
            app.copy(&password, "Пароль");
        }
        if let Some(id) = self.toggle_favorite
            && let Some(item) = vault.get(id)
        {
            let mut updated = item.clone();
            updated.favorite = !updated.favorite;
            let verdict = if updated.favorite {
                "добавлена в избранное"
            } else {
                "убрана из избранного"
            };
            match vault.update(updated) {
                Ok(()) => app.status = format!("Запись {verdict}."),
                Err(err) => app.status = format!("Не удалось изменить избранное: {err}"),
            }
        }
        if let Some(id) = self.delete {
            app.confirm_delete = Some(id);
        }
    }
}

/// Действия из карточки, применяемые после отрисовки (И-4).
#[derive(Default)]
struct CardIntents {
    edit: Option<Uuid>,
    delete: Option<Uuid>,
    toggle_favorite: Option<Uuid>,
    copy_username: Option<String>,
    copy_password: Option<String>,
    copy_totp: Option<String>,
    copy_url: Option<String>,
    copy_field: Option<(String, String)>,
    restore_version: Option<(Uuid, usize)>,
}

impl CardIntents {
    fn apply(self, app: &mut App, vault: &mut Vault) {
        if let Some(id) = self.edit
            && let Some(item) = vault.get(id)
        {
            app.draft = Some(Draft::from_item(item));
        }
        if let Some(id) = self.delete {
            app.confirm_delete = Some(id);
        }
        if let Some(id) = self.toggle_favorite
            && let Some(item) = vault.get(id)
        {
            let mut updated = item.clone();
            updated.favorite = !updated.favorite;
            let _ = vault.update(updated);
        }
        if let Some(username) = self.copy_username {
            app.copy(&username, "Логин");
        }
        if let Some(password) = self.copy_password {
            app.copy(&password, "Пароль");
        }
        if let Some(code) = self.copy_totp {
            app.copy(&code, "Код TOTP");
        }
        if let Some(url) = self.copy_url {
            app.copy(&url, "Сайт");
        }
        if let Some((label, value)) = self.copy_field {
            app.copy(&value, &label);
        }
        if let Some((id, index)) = self.restore_version {
            match vault.restore_password_version(id, index) {
                Ok(()) => app.status = "Прежний пароль снова текущий.".to_string(),
                Err(err) => app.status = format!("Не удалось вернуть пароль: {err}"),
            }
        }
    }
}

/// Время жизни кнопки «Вернуть» после удаления (И-6).
const UNDO_TTL: Duration = Duration::from_secs(10);

//! Палитра быстрого доступа.

use crate::*;

impl App {
    /// Палитра быстрого доступа (как Quick Access у 1Password): открывается
    /// глобальной клавишей из любого окна, фильтрует записи, сортирует
    /// подходящие целевому окну наверх и по Enter возвращает фокус окну
    /// и печатает туда логин и пароль.
    pub(crate) fn quick_access_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if self.quick_access.is_none() {
            self.quick_prefill = None;
            return;
        }
        self.pull_quick_prefill(vault);
        let Some(quick) = self.quick_access.as_mut() else {
            return;
        };

        // Записи по фильтру; подходящие целевому окну поднимаются наверх.
        let mut rows: Vec<(Uuid, String, String, ItemKind, MatchStrength)> = vault
            .items()
            .iter()
            .filter(|item| item.matches(&quick.query))
            .map(|item| {
                let strength = quick
                    .target_title
                    .as_deref()
                    .map(|title| item.url_match_strength(title))
                    .unwrap_or(MatchStrength::None);
                (
                    item.id,
                    item.title.clone(),
                    item.username.clone(),
                    item.kind,
                    strength,
                )
            })
            .collect();
        rows.sort_by(|a, b| {
            b.4.cmp(&a.4).then_with(|| {
                let login = |kind: ItemKind| matches!(kind, ItemKind::Login);
                login(b.3).cmp(&login(a.3))
            })
        });
        rows.truncate(10);
        if quick.cursor >= rows.len() {
            quick.cursor = rows.len().saturating_sub(1);
        }

        // Навигация клавишами работает и в поле фильтра, и просто в окне.
        let mut commit = None;
        let (go_up, go_down, enter, copy_password) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::C),
            )
        });
        if enter {
            commit = Some(quick.cursor);
        }
        if go_up && quick.cursor > 0 {
            quick.cursor -= 1;
        }
        if go_down && quick.cursor + 1 < rows.len() {
            quick.cursor += 1;
        }

        let mut open = true;
        egui::Window::new("Быстрый доступ")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0))
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut quick.query)
                        .hint_text("Название, логин или сайт…")
                        .desired_width(f32::INFINITY),
                );
                if quick.focus {
                    edit.request_focus();
                    quick.focus = false;
                }
                ui.add_space(4.0);

                if rows.is_empty() {
                    ui.label(egui::RichText::new("Ничего не найдено").weak());
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("quick_rows")
                        .auto_shrink([false, false])
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (idx, (_, title, username, kind, strength)) in
                                rows.iter().enumerate()
                            {
                                let mut second = if username.is_empty() {
                                    kind.label().to_string()
                                } else {
                                    username.clone()
                                };
                                match strength {
                                    MatchStrength::Strong => {
                                        second.push_str(" · сайт совпадает");
                                    }
                                    MatchStrength::Weak => {
                                        second.push_str(" · похоже по названию");
                                    }
                                    MatchStrength::None => {}
                                }
                                let selected = quick.cursor == idx;
                                if ui
                                    .selectable_label(selected, format!("{title}\n{second}"))
                                    .clicked()
                                {
                                    commit = Some(idx);
                                }
                            }
                        });
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(match quick.target_title.as_deref() {
                        Some(title) if !title.is_empty() => {
                            format!("Целевое окно: {title}")
                        }
                        _ => "Целевое окно неизвестно — ввод пойдёт в активное окно".to_string(),
                    })
                    .small()
                    .weak(),
                );
                ui.label(
                    egui::RichText::new(
                        "Enter — ввести · Ctrl+C — копировать пароль · Esc — закрыть",
                    )
                    .small()
                    .weak(),
                );
            });
        if !open {
            self.quick_access = None;
            return;
        }

        // Действие по выбранной строке: копия пароля или автоввод в окно.
        let Some(idx) = commit else {
            if copy_password && !rows.is_empty() {
                let idx = quick.cursor;
                let target_hwnd = quick.target_hwnd;
                let (_, title, username, _, _) = &rows[idx];
                if let Some(item) = vault.get(rows[idx].0) {
                    let password = item.password.clone();
                    if password.is_empty() {
                        self.status = format!("У записи «{title}» нет пароля.");
                    } else {
                        let _ = (target_hwnd, username);
                        self.copy(&password, "Пароль");
                        self.quick_access = None;
                    }
                }
            }
            return;
        };
        let (id, title, username, _, _) = rows[idx].clone();
        let target_hwnd = quick.target_hwnd;
        let Some(item) = vault.get(id) else {
            self.quick_access = None;
            return;
        };
        let username = if item.username.is_empty() {
            username
        } else {
            item.username.clone()
        };
        let password = item.password.clone();
        if username.is_empty() && password.is_empty() {
            self.status = format!("У записи «{title}» нет логина и пароля.");
            self.quick_access = None;
            return;
        }
        self.status = format!("Возвращаюсь в целевое окно: {title}…");
        autotype::run_quick(
            ctx.clone(),
            target_hwnd,
            username,
            Zeroizing::new(password),
            title,
        );
        self.quick_access = None;
        self.quick_prefill = None;
    }

    /// Забирает адрес страницы из фонового чтения и подставляет его в поиск.
    pub(crate) fn pull_quick_prefill(&mut self, vault: &Vault) {
        let Some(receiver) = self.quick_prefill.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(query) => {
                let Some(quick) = self.quick_access.as_mut() else {
                    return;
                };
                if !quick.query.is_empty() || query.is_empty() {
                    return;
                }
                // Записей для полного домена нет — ищем по имени сайта:
                // fialkaspb.ru → fialkaspb.
                let exact = vault.items().iter().any(|item| item.matches(&query));
                quick.query = if exact { query } else { site_label(&query) };
                quick.cursor = 0;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => self.quick_prefill = Some(receiver),
            Err(_) => {}
        }
    }
}

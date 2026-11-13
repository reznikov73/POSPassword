//! Оконный клиент «Хранитель» на eframe/egui.
//!
//! Вся криптография и работа с файлом — в `vault-core`; здесь только UI,
//! буфер обмена с автоочисткой и автоблокировка.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use uuid::Uuid;
use vault_core::{
    Field, GeneratorOptions, ImportFormat, Item, ItemKind, Vault, generate_password, import_file,
};
use zeroize::Zeroize;

const APP_TITLE: &str = "Хранитель";
const AUTO_LOCK: Duration = Duration::from_secs(5 * 60);
const CLIPBOARD_TTL: Duration = Duration::from_secs(30);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1020.0, 680.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title(APP_TITLE),
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Open,
    Create,
}

struct Draft {
    id: Option<Uuid>,
    kind: ItemKind,
    title: String,
    username: String,
    password: String,
    url: String,
    notes: String,
    fields: Vec<Field>,
    show_plain: bool,
}

impl Draft {
    fn new_item() -> Self {
        Self::from_kind(ItemKind::Login)
    }

    fn from_kind(kind: ItemKind) -> Self {
        Self {
            id: None,
            kind,
            title: String::new(),
            username: String::new(),
            password: String::new(),
            url: String::new(),
            notes: String::new(),
            fields: kind.template(),
            show_plain: true,
        }
    }

    fn from_item(item: &Item) -> Self {
        Self {
            id: Some(item.id),
            kind: item.kind,
            title: item.title.clone(),
            username: item.username.clone(),
            password: item.password.clone(),
            url: item.url.clone(),
            notes: item.notes.clone(),
            fields: item.fields.clone(),
            show_plain: false,
        }
    }

    fn build(&self) -> Item {
        let mut item = Item::new(self.kind, self.title.trim());
        if let Some(id) = self.id {
            item.id = id;
        }
        item.username = self.username.trim().to_string();
        item.password = self.password.clone();
        item.url = self.url.trim().to_string();
        item.notes = self.notes.clone();
        item.fields = self.fields.clone();
        item
    }
}

/// Заготовка полей для нового типа записи.
fn template_fields(kind: ItemKind) -> Vec<Field> {
    kind.template()
}

enum DraftOutcome {
    Idle,
    Cancel,
    Save { item: Box<Item>, is_new: bool },
}

struct App {
    mode: Mode,
    path: String,
    password: String,
    confirm: String,
    vault: Option<Vault>,
    query: String,
    selected: Option<Uuid>,
    draft: Option<Draft>,
    confirm_delete: Option<Uuid>,
    status: String,
    error: Option<String>,
    reveal_password: bool,
    clipboard: Option<(Instant, String)>,
    last_activity: Instant,
    import_open: bool,
    import_path: String,
    import_format: ImportFormat,
    import_result: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_theme(&cc.egui_ctx);
        install_fonts(&cc.egui_ctx);
        Self {
            mode: Mode::Open,
            path: default_vault_path(),
            password: String::new(),
            confirm: String::new(),
            vault: None,
            query: String::new(),
            selected: None,
            draft: None,
            confirm_delete: None,
            status: "Введите мастер-пароль, чтобы открыть хранилище.".to_string(),
            error: None,
            reveal_password: false,
            clipboard: None,
            last_activity: Instant::now(),
            import_open: false,
            import_path: String::new(),
            import_format: ImportFormat::Auto,
            import_result: None,
        }
    }

    fn lock(&mut self, reason: &str) {
        self.vault = None;
        self.draft = None;
        self.confirm_delete = None;
        self.selected = None;
        self.query.clear();
        self.reveal_password = false;
        self.mode = Mode::Open;
        self.password.zeroize();
        self.confirm.zeroize();
        self.error = None;
        self.status = reason.to_string();
        self.import_open = false;
        self.import_result = None;
    }

    fn copy(&mut self, text: &str, label: &str) {
        if text.is_empty() {
            self.status = format!("{label}: нечего копировать");
            return;
        }
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string())) {
            Ok(()) => {
                self.clipboard = Some((Instant::now(), text.to_string()));
                self.status = format!("{label} скопирован. Буфер очистится через 30 секунд.");
            }
            Err(err) => self.status = format!("Не удалось скопировать: {err}"),
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if let Some((at, secret)) = self.clipboard.clone()
            && at.elapsed() >= CLIPBOARD_TTL
        {
            if let Ok(mut cb) = arboard::Clipboard::new()
                && cb.get_text().map(|t| t == secret).unwrap_or(false)
            {
                let _ = cb.set_text(String::new());
            }
            self.clipboard = None;
        }

        if self.vault.is_some() && self.last_activity.elapsed() >= AUTO_LOCK {
            self.lock("Заблокировано по бездействию. Введите мастер-пароль снова.");
        }

        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn track_activity(&mut self, ctx: &egui::Context) {
        let active = ctx.input(|i| i.pointer.any_click() || !i.events.is_empty());
        if active {
            self.last_activity = Instant::now();
        }
    }

    fn submit_unlock(&mut self) {
        self.error = None;
        let path = PathBuf::from(self.path.trim());
        let result = match self.mode {
            Mode::Open => Vault::open(&path, &self.password),
            Mode::Create => {
                if self.password != self.confirm {
                    self.error = Some("Пароли не совпадают.".into());
                    return;
                }
                Vault::create(&path, &self.password)
            }
        };

        match result {
            Ok(vault) => {
                self.status = match self.mode {
                    Mode::Open => "Хранилище открыто.".to_string(),
                    Mode::Create => "Хранилище создано.".to_string(),
                };
                self.vault = Some(vault);
                self.password.zeroize();
                self.confirm.zeroize();
                self.mode = Mode::Open;
                self.last_activity = Instant::now();
                self.error = None;
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    fn unlock_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(36.0);
        ui.vertical_centered(|ui| {
            ui.heading(APP_TITLE);
            ui.label(
                egui::RichText::new("Локальное хранилище паролей · Windows")
                    .weak()
                    .small(),
            );
            ui.add_space(22.0);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(440.0);
                ui.add_space(4.0);

                ui.label("Файл хранилища");
                ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(f32::INFINITY));
                ui.add_space(8.0);

                ui.label("Мастер-пароль");
                let password_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );

                if self.mode == Mode::Create {
                    ui.add_space(8.0);
                    ui.label("Повторите мастер-пароль");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.confirm)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    );
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let action = match self.mode {
                        Mode::Open => "Открыть",
                        Mode::Create => "Создать хранилище",
                    };
                    let clicked = ui.button(action).clicked()
                        || (password_edit.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if clicked {
                        self.submit_unlock();
                    }

                    match self.mode {
                        Mode::Open => {
                            if ui.button("Создать новый…").clicked() {
                                self.mode = Mode::Create;
                                self.error = None;
                                self.password.zeroize();
                                self.confirm.zeroize();
                            }
                        }
                        Mode::Create => {
                            if ui.button("Назад").clicked() {
                                self.mode = Mode::Open;
                                self.error = None;
                                self.confirm.zeroize();
                            }
                        }
                    }
                });

                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error);
                }
                ui.add_space(4.0);
            });

            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(
                    "Argon2id (64 МиБ, 3 итерации) + XChaCha20-Poly1305. Пароль не покидает машину.",
                )
                .weak()
                .small(),
            );
        });
    }

    fn main_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut vault) = self.vault.take() else {
            return;
        };
        let mut lock_requested = false;

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
                if ui.button("Импорт…").clicked() {
                    self.import_open = true;
                    self.import_result = None;
                }
                ui.label(format!("Записей: {}", vault.items().len()));
            });
        });
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(340.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text("Поиск")
                                .desired_width(250.0),
                        );
                        if ui.button("+").on_hover_text("Новая запись").clicked() {
                            self.draft = Some(Draft::new_item());
                        }
                    });
                    ui.add_space(6.0);

                    let items: Vec<(Uuid, String, String, ItemKind)> = vault
                        .search(&self.query)
                        .into_iter()
                        .map(|i| (i.id, i.title.clone(), i.username.clone(), i.kind))
                        .collect();

                    egui::ScrollArea::vertical()
                        .id_salt("item_list")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if items.is_empty() {
                                ui.label(egui::RichText::new("Ничего не найдено").weak());
                            }
                            for (id, title, username, kind) in &items {
                                let label = if !username.is_empty() {
                                    format!("{title}\n{username}")
                                } else if matches!(kind, ItemKind::Login | ItemKind::SecureNote) {
                                    title.clone()
                                } else {
                                    format!("{title}\n{}", kind.label())
                                };
                                if ui
                                    .selectable_label(self.selected == Some(*id), label)
                                    .clicked()
                                {
                                    self.selected = Some(*id);
                                    self.reveal_password = false;
                                }
                            }
                        });
                },
            );

            ui.separator();

            ui.vertical(|ui| {
                let selected_item: Option<Item> =
                    self.selected.and_then(|id| vault.get(id)).cloned();

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
                                },
                            );
                        });
                        ui.label(egui::RichText::new(item.kind.label()).weak().small());
                        ui.add_space(10.0);

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
                                        let shown = if self.reveal_password {
                                            item.password.clone()
                                        } else {
                                            "•".repeat(item.password.chars().count().min(24))
                                        };
                                        ui.monospace(shown);
                                        let toggle = if self.reveal_password {
                                            "Скрыть"
                                        } else {
                                            "Показать"
                                        };
                                        if ui.small_button(toggle).clicked() {
                                            self.reveal_password = !self.reveal_password;
                                        }
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(&item.password, "Пароль");
                                        }
                                    });
                                    ui.end_row();
                                }

                                if !item.url.is_empty() {
                                    ui.label("Сайт");
                                    ui.horizontal(|ui| {
                                        ui.monospace(&item.url);
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(&item.url, "Сайт");
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
                                        let shown = if field.sensitive && !self.reveal_password {
                                            "•".repeat(field.value.chars().count().min(20))
                                        } else {
                                            field.value.clone()
                                        };
                                        ui.monospace(shown);
                                        if field.sensitive {
                                            let toggle = if self.reveal_password {
                                                "Скрыть"
                                            } else {
                                                "Показать"
                                            };
                                            if ui.small_button(toggle).clicked() {
                                                self.reveal_password = !self.reveal_password;
                                            }
                                        }
                                        if ui.small_button("Копировать").clicked() {
                                            self.copy(&field.value, &field.label);
                                        }
                                    });
                                    ui.end_row();
                                }
                            });

                        if !item.notes.is_empty() {
                            ui.add_space(12.0);
                            ui.label("Заметки");
                            ui.label(&item.notes);
                        }
                    });
            });
        });

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
                        if self.selected == Some(id) {
                            self.selected = None;
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

        match draw_draft(ui.ctx(), &mut self.draft) {
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
                        self.selected = Some(item.id);
                        self.draft = None;
                        self.status = "Сохранено.".to_string();
                    }
                    Err(err) => self.status = format!("Ошибка сохранения: {err}"),
                }
            }
        }

        if lock_requested {
            self.lock("Заблокировано.");
        } else {
            self.vault = Some(vault);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.tick(ui.ctx());
        self.track_activity(ui.ctx());

        let status_height = 34.0;
        let content_height = (ui.available_height() - status_height).max(0.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        if self.vault.is_some() {
                            self.main_ui(ui);
                        } else {
                            self.unlock_ui(ui);
                        }
                    });
            },
        );

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.status).small().weak());
            if self.clipboard.is_some() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("буфер обмена будет очищен автоматически")
                            .small()
                            .weak(),
                    );
                });
            }
        });
    }
}

fn draw_draft(ctx: &egui::Context, draft: &mut Option<Draft>) -> DraftOutcome {
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
                                    .desired_width(250.0)
                                    .password(!d.show_plain),
                            );
                            if ui.small_button("Показать").clicked() {
                                d.show_plain = !d.show_plain;
                            }
                            if ui.small_button("Сгенерировать").clicked() {
                                d.password = generate_password(&GeneratorOptions::default())
                                    .unwrap_or_default();
                                d.show_plain = true;
                            }
                        });
                        ui.end_row();

                        ui.label("Сайт");
                        ui.add(egui::TextEdit::singleline(&mut d.url).desired_width(380.0));
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

fn install_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(24, 26, 31);
    visuals.window_fill = egui::Color32::from_rgb(31, 34, 40);
    visuals.faint_bg_color = egui::Color32::from_rgb(38, 41, 48);
    visuals.extreme_bg_color = egui::Color32::from_rgb(17, 19, 23);
    ctx.set_visuals(visuals);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        (
            "C:\\Windows\\Fonts\\segoeui.ttf",
            egui::FontFamily::Proportional,
        ),
        (
            "C:\\Windows\\Fonts\\consola.ttf",
            egui::FontFamily::Monospace,
        ),
    ];

    for (path, family) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let name = format!("sys:{path}");
            fonts
                .font_data
                .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(family).or_default().insert(0, name);
        }
    }

    ctx.set_fonts(fonts);
}

fn default_vault_path() -> String {
    // В некоторых окружениях APPDATA не задан; тогда собираем путь от профиля.
    let base = std::env::var("APPDATA")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|profile| format!("{profile}\\AppData\\Roaming"))
        })
        .unwrap_or_else(|| ".".to_string());

    PathBuf::from(base)
        .join("Hranitel")
        .join("vault.opvault")
        .display()
        .to_string()
}

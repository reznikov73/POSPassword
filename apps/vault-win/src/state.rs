//! Состояние приложения: структура App, блокировка, жизненный цикл.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use uuid::Uuid;
use vault_core::{
    ExportFormat, Field, GeneratorOptions, ImportFormat, Item, ItemKind, PassphraseOptions, Vault,
    Wordlist, normalize_host, protected_wrapping_key, registrable_domain, sidecar_path,
};
use zeroize::{Zeroize, Zeroizing};

use crate::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Open,
    Create,
}

/// Режим генератора паролей: случайные символы или запоминаемая фраза.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenMode {
    Chars,
    Phrase,
}

/// Категория для фильтра списка.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum KindGroup {
    All,
    Logins,
    Cards,
    Accounts,
    Notes,
    Documents,
    Keys,
}

impl KindGroup {
    pub(crate) fn all() -> [KindGroup; 7] {
        [
            KindGroup::All,
            KindGroup::Logins,
            KindGroup::Cards,
            KindGroup::Accounts,
            KindGroup::Notes,
            KindGroup::Documents,
            KindGroup::Keys,
        ]
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            KindGroup::All => "Все",
            KindGroup::Logins => "Логины",
            KindGroup::Cards => "Карты",
            KindGroup::Accounts => "Счета",
            KindGroup::Notes => "Заметки",
            KindGroup::Documents => "Документы",
            KindGroup::Keys => "Ключи",
        }
    }

    pub(crate) fn matches(self, kind: ItemKind) -> bool {
        self == KindGroup::All || group_of(kind) == self
    }
}

pub(crate) fn group_of(kind: ItemKind) -> KindGroup {
    match kind {
        ItemKind::Login => KindGroup::Logins,
        ItemKind::Card => KindGroup::Cards,
        ItemKind::BankAccount => KindGroup::Accounts,
        ItemKind::SecureNote => KindGroup::Notes,
        ItemKind::ApiKey | ItemKind::Wifi | ItemKind::Pin | ItemKind::DigitalSignature => {
            KindGroup::Keys
        }
        ItemKind::Passport
        | ItemKind::ForeignPassport
        | ItemKind::Snils
        | ItemKind::Inn
        | ItemKind::DriverLicense
        | ItemKind::MedicalPolicy
        | ItemKind::Insurance
        | ItemKind::Property
        | ItemKind::Health
        | ItemKind::Vehicle => KindGroup::Documents,
    }
}

pub(crate) struct Draft {
    pub(crate) id: Option<Uuid>,
    pub(crate) kind: ItemKind,
    pub(crate) title: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) url: String,
    /// Дополнительные сайты записи — по одному адресу в строке.
    pub(crate) urls: String,
    pub(crate) notes: String,
    pub(crate) fields: Vec<Field>,
    pub(crate) show_plain: bool,
}

impl Draft {
    pub(crate) fn new_item() -> Self {
        Self::from_kind(ItemKind::Login)
    }

    pub(crate) fn from_kind(kind: ItemKind) -> Self {
        Self {
            id: None,
            kind,
            title: String::new(),
            username: String::new(),
            password: String::new(),
            url: String::new(),
            urls: String::new(),
            notes: String::new(),
            fields: kind.template(),
            show_plain: true,
        }
    }

    pub(crate) fn from_item(item: &Item) -> Self {
        Self {
            id: Some(item.id),
            kind: item.kind,
            title: item.title.clone(),
            username: item.username.clone(),
            password: item.password.clone(),
            url: item.url.clone(),
            urls: item.urls.join("\n"),
            notes: item.notes.clone(),
            fields: item.fields.clone(),
            show_plain: false,
        }
    }

    pub(crate) fn build(&self) -> Item {
        let mut item = Item::new(self.kind, self.title.trim());
        if let Some(id) = self.id {
            item.id = id;
        }
        item.username = self.username.trim().to_string();
        item.password = self.password.clone();
        item.url = self.url.trim().to_string();
        item.urls = self
            .urls
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        item.notes = self.notes.clone();
        item.fields = self.fields.clone();
        item
    }
}

/// Заготовка полей для нового типа записи.
pub(crate) fn template_fields(kind: ItemKind) -> Vec<Field> {
    kind.template()
}

pub(crate) enum DraftOutcome {
    Idle,
    Cancel,
    Save { item: Box<Item>, is_new: bool },
}

/// Состояние палитры быстрого доступа: фильтр, курсор и целевое окно,
/// из которого палитра вызвана.
pub(crate) struct QuickAccess {
    pub(crate) query: String,
    /// Выбранная строка списка.
    pub(crate) cursor: usize,
    /// Заголовок целевого окна — по нему сортируются записи.
    pub(crate) target_title: Option<String>,
    /// Дескриптор целевого окна для возврата фокуса (0 — неизвестно).
    pub(crate) target_hwnd: isize,
    /// Поставить курсор в фильтр при открытии.
    pub(crate) focus: bool,
}

/// Запрос расширения, ожидающий решения пользователя.
pub(crate) enum BrowserAsk {
    /// Разрешение на доступ к сайту.
    Access {
        domain: String,
        request: browser::BrowserRequest,
        reply: Sender<String>,
    },
    /// Сохранить новую запись из браузера.
    SaveNew {
        url: String,
        username: String,
        password: Zeroizing<String>,
        title: String,
        reply: Sender<String>,
    },
    /// Обновить пароль существующей записи из браузера.
    SaveUpdate {
        id: Uuid,
        url: String,
        username: String,
        password: Zeroizing<String>,
        title: String,
        reply: Sender<String>,
    },
}

impl BrowserAsk {
    /// Отвечает отказом, не показывая диалог (блокировка, закрытие).
    pub(crate) fn deny(self, reason: &str) {
        let reply = match self {
            BrowserAsk::Access { reply, .. }
            | BrowserAsk::SaveNew { reply, .. }
            | BrowserAsk::SaveUpdate { reply, .. } => reply,
        };
        let _ = reply.send(browser::error_json(reason));
    }
}

/// Решение пользователя по запросу расширения.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserOutcome {
    AllowOnce,
    AllowAlways,
    Deny,
    Save,
    SaveUpdate,
    SaveAsNew,
}

/// Экран приложения: вход или работа с открытым хранилищем.
/// Поля принадлежат только своему экрану — компилятор не даст прочитать
/// мастер-пароль после разблокировки или список записей на экране входа.
pub(crate) enum Screen {
    Unlock {
        mode: Mode,
        password: String,
        confirm: String,
        /// Фоновый запрос Windows Hello; результат забирается в `tick`.
        hello_pending: Option<Receiver<Result<Vault, String>>>,
        /// Поставить курсор в поле мастер-пароля (браузер или трей просят доступ).
        focus_password: bool,
    },
    Main {
        /// `None` — только на мгновение внутри `main_ui` (шаблон take/put,
        /// чтобы диалоги могли занимать и приложение, и хранилище).
        vault: Option<Vault>,
        selected: Option<Uuid>,
        query: String,
        kind_filter: KindGroup,
        reveal_password: bool,
    },
}

impl Screen {
    /// Открыто ли хранилище (экран работы).
    pub(crate) fn is_main(&self) -> bool {
        matches!(self, Screen::Main { .. })
    }

    /// Хранилище по ссылке, если экран работы.
    pub(crate) fn vault(&self) -> Option<&Vault> {
        match self {
            Screen::Main { vault, .. } => vault.as_ref(),
            Screen::Unlock { .. } => None,
        }
    }

    /// Забрать хранилище на время кадра (вернуть через `put_vault`).
    pub(crate) fn take_vault(&mut self) -> Option<Vault> {
        match self {
            Screen::Main { vault, .. } => vault.take(),
            Screen::Unlock { .. } => None,
        }
    }

    /// Хранилище с правом изменения, если экран работы.
    pub(crate) fn vault_mut(&mut self) -> Option<&mut Vault> {
        match self {
            Screen::Main { vault, .. } => vault.as_mut(),
            Screen::Unlock { .. } => None,
        }
    }

    /// Вернуть хранилище, взятое через `take_vault`.
    pub(crate) fn put_vault(&mut self, back: Vault) {
        if let Screen::Main { vault, .. } = self {
            *vault = Some(back);
        }
    }

    /// Выбранная запись (или `None` на экране входа).
    pub(crate) fn selected(&self) -> Option<Uuid> {
        match self {
            Screen::Main { selected, .. } => *selected,
            Screen::Unlock { .. } => None,
        }
    }

    /// Выбранная запись с правом изменения (для диалогов аудита).
    pub(crate) fn selected_mut(&mut self) -> Option<&mut Option<Uuid>> {
        match self {
            Screen::Main { selected, .. } => Some(selected),
            Screen::Unlock { .. } => None,
        }
    }

    pub(crate) fn set_selected(&mut self, id: Option<Uuid>) {
        if let Screen::Main { selected, .. } = self {
            *selected = id;
        }
    }

    /// Поисковый запрос — только на экране работы.
    pub(crate) fn query_mut(&mut self) -> Option<&mut String> {
        match self {
            Screen::Main { query, .. } => Some(query),
            Screen::Unlock { .. } => None,
        }
    }

    /// Показаны ли скрытые значения (пароли) в карточке.
    pub(crate) fn reveal_password(&self) -> bool {
        match self {
            Screen::Main {
                reveal_password, ..
            } => *reveal_password,
            Screen::Unlock { .. } => false,
        }
    }

    pub(crate) fn set_reveal_password(&mut self, on: bool) {
        if let Screen::Main {
            reveal_password, ..
        } = self
        {
            *reveal_password = on;
        }
    }

    /// Активная категория списка.
    pub(crate) fn kind_filter(&self) -> KindGroup {
        match self {
            Screen::Main { kind_filter, .. } => *kind_filter,
            Screen::Unlock { .. } => KindGroup::All,
        }
    }

    pub(crate) fn set_kind_filter(&mut self, group: KindGroup) {
        if let Screen::Main { kind_filter, .. } = self {
            *kind_filter = group;
        }
    }

    /// Мастер-пароль для поля ввода — только на экране входа.
    pub(crate) fn unlock_password_mut(&mut self) -> Option<&mut String> {
        match self {
            Screen::Unlock { password, .. } => Some(password),
            Screen::Main { .. } => None,
        }
    }

    /// Повтор пароля при создании — только на экране входа.
    pub(crate) fn unlock_confirm_mut(&mut self) -> Option<&mut String> {
        match self {
            Screen::Unlock { confirm, .. } => Some(confirm),
            Screen::Main { .. } => None,
        }
    }

    /// Режим экрана входа: открыть существующее или создать новое.
    pub(crate) fn unlock_mode(&self) -> Option<Mode> {
        match self {
            Screen::Unlock { mode, .. } => Some(*mode),
            Screen::Main { .. } => None,
        }
    }

    pub(crate) fn set_unlock_mode(&mut self, mode: Mode) {
        if let Screen::Unlock { mode: slot, .. } = self {
            *slot = mode;
        }
    }

    /// Запрос Windows Hello, ожидающий подтверждения.
    pub(crate) fn take_hello_pending(&mut self) -> Option<Receiver<Result<Vault, String>>> {
        match self {
            Screen::Unlock { hello_pending, .. } => hello_pending.take(),
            Screen::Main { .. } => None,
        }
    }

    pub(crate) fn set_hello_pending(&mut self, rx: Option<Receiver<Result<Vault, String>>>) {
        if let Screen::Unlock { hello_pending, .. } = self {
            *hello_pending = rx;
        }
    }

    pub(crate) fn hello_pending_is_some(&self) -> bool {
        matches!(
            self,
            Screen::Unlock {
                hello_pending: Some(_),
                ..
            }
        )
    }

    /// Поставить курсор в поле мастер-пароля при следующем кадре.
    pub(crate) fn set_focus_password(&mut self, on: bool) {
        if let Screen::Unlock { focus_password, .. } = self {
            *focus_password = on;
        }
    }

    pub(crate) fn focus_password(&self) -> bool {
        matches!(
            self,
            Screen::Unlock {
                focus_password: true,
                ..
            }
        )
    }
}

pub(crate) struct App {
    pub(crate) path: String,
    /// Запуск с ключом --tray: окно спрятать после первого кадра.
    pub(crate) start_hidden: bool,
    /// Момент повторного скрытия: окно успевает появиться после первого кадра.
    pub(crate) hide_after: Option<Instant>,
    /// Дескриптор главного окна: прячем и показываем его напрямую через Win32.
    pub(crate) main_hwnd: isize,
    /// Сворачивать в трей при закрытии окна.
    pub(crate) hide_to_tray: bool,
    /// Запускать при входе в Windows.
    pub(crate) autostart: bool,
    /// Команды значка в трее.
    pub(crate) tray_commands: Option<std::sync::mpsc::Receiver<tray::TrayCommand>>,
    /// Разрешено закрыть программу по-настоящему (пункт «Выход»).
    pub(crate) allow_exit: bool,
    /// Последнее чужое окно на переднем плане — цель «Быстрого доступа» из трея.
    pub(crate) last_target: Option<(isize, String)>,
    /// Запрос браузера, ждущий разблокировки хранилища.
    pub(crate) pending_browser: Option<(browser::BrowserRequest, Sender<String>)>,
    /// Запрос быстрого доступа, ждущий разблокировки.
    pub(crate) pending_quick: Option<autotype::QuickRequest>,
    /// Экран: вход или работа с открытым хранилищем.
    pub(crate) screen: Screen,
    pub(crate) draft: Option<Draft>,
    pub(crate) confirm_delete: Option<Uuid>,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) clipboard: Option<(Instant, String)>,
    pub(crate) last_activity: Instant,
    pub(crate) import_open: bool,
    pub(crate) import_path: String,
    pub(crate) import_format: ImportFormat,
    pub(crate) import_result: Option<String>,
    pub(crate) theme: Theme,
    /// Окно встроенной справки, открывается по F1.
    pub(crate) help_open: bool,
    /// Курсор должен прыгнуть в поиск на следующем кадре (Ctrl+F).
    pub(crate) focus_search: bool,
    /// Диалог смены мастер-пароля.
    pub(crate) password_open: bool,
    pub(crate) password_current: String,
    pub(crate) password_new: String,
    pub(crate) password_confirm: String,
    pub(crate) password_error: Option<String>,
    /// Диалог экспорта.
    pub(crate) export_open: bool,
    pub(crate) export_format: ExportFormat,
    pub(crate) export_path: String,
    pub(crate) export_password: String,
    pub(crate) export_result: Option<String>,
    /// Панель аудита хранилища.
    pub(crate) audit_open: bool,
    /// Сочетание клавиш автоввода.
    pub(crate) hotkey: autotype::Hotkey,
    /// Сочетание клавиш быстрого доступа.
    pub(crate) quick_hotkey: autotype::Hotkey,
    /// Диалог настроек.
    pub(crate) settings_open: bool,
    pub(crate) hotkey_draft: String,
    pub(crate) hotkey_error: Option<String>,
    /// Идёт запись нового сочетания: следующее нажатие станет горячей клавишей.
    pub(crate) hotkey_capture: bool,
    /// Поставить курсор в поле сочетания при открытии настроек.
    pub(crate) hotkey_focus: bool,
    /// Те же поля для сочетания быстрого доступа.
    pub(crate) quick_draft: String,
    pub(crate) quick_error: Option<String>,
    pub(crate) quick_capture: bool,
    /// Палитра быстрого доступа, открыта пока поле не None.
    pub(crate) quick_access: Option<QuickAccess>,
    /// Адрес страницы, прочитанный в фоне: палитра подставит сайт в поиск.
    pub(crate) quick_prefill: Option<std::sync::mpsc::Receiver<String>>,
    /// Момент, когда надо выполнить автоввод после кнопки с отсчётом.
    pub(crate) autotype_countdown: Option<Instant>,
    /// Сообщение автоввода на несколько секунд поверх окна.
    pub(crate) autotype_notice: Option<(String, Instant)>,
    /// Диалог генератора паролей.
    pub(crate) gen_open: bool,
    pub(crate) gen_mode: GenMode,
    /// Язык, для которого сейчас составлен образец фразы: по нему видно,
    /// что образец устарел после смены языка (сравнивать текст нельзя —
    /// после смены языка образец пересоздавался каждый кадр).
    pub(crate) gen_sample_lang: Option<Wordlist>,
    pub(crate) gen_opts: GeneratorOptions,
    pub(crate) gen_phrase: PassphraseOptions,
    pub(crate) gen_result: String,
    /// Диалог быстрой вставки логина и пароля.
    pub(crate) quick_open: bool,
    pub(crate) quick_text: String,
    /// Поля быстрой вставки — редактируются после разбора.
    pub(crate) quick_title: String,
    pub(crate) quick_username: String,
    pub(crate) quick_password: String,
    pub(crate) quick_url: String,
    /// Диалог массового добавления описаний.
    pub(crate) desc_open: bool,
    /// Запрос на описание одной записи из карточки.
    pub(crate) describe_one: Option<Uuid>,
    /// Задания от моста к браузерному расширению.
    pub(crate) browser_jobs: Receiver<browser::BrowserJob>,
    /// Запрос расширения, ждущий решения пользователя в диалоге.
    pub(crate) browser_ask: Option<BrowserAsk>,
    /// Домены, к которым расширение имеет постоянный доступ.
    pub(crate) browser_domains: HashSet<String>,
    /// Разрешено ли расширение браузера обращаться к хранилищу.
    pub(crate) browser_enabled: bool,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = load_settings();
        let theme = settings.theme;
        let hotkey = settings.hotkey;
        let quick_hotkey = settings.quick_hotkey;
        let hide_to_tray = settings.tray;
        let autostart = autostart_enabled();
        let start_hidden = std::env::args().any(|arg| arg == "--tray");
        let main_hwnd = main_window_hwnd(cc);
        let startup_path = settings
            .vault_path
            .clone()
            .unwrap_or_else(default_vault_path);
        theme.apply(&cc.egui_ctx);
        install_fonts(&cc.egui_ctx);

        // Мост к браузерному расширению: именованный канал и список
        // разрешённых сайтов живут, пока запущено приложение.
        let browser_jobs = browser::start(cc.egui_ctx.clone());
        let browser_domains = browser::load_domains(&app_dir());
        let browser_enabled = settings.browser;

        // Горячие клавиши живут, пока запущено приложение: автоввод печатает
        // логин и пароль, быстрый доступ поднимает палитру выбора записи.
        let mut status = match autotype::start(cc.egui_ctx.clone(), hotkey, quick_hotkey) {
            Ok(()) => "Введите мастер-пароль, чтобы открыть хранилище.".to_string(),
            Err(err) => format!(
                "Автоввод ({}) недоступен: {err}. Смените комбинацию в настройках. Введите мастер-пароль, чтобы открыть хранилище.",
                hotkey.label()
            ),
        };
        if !autotype::is_quick_ready() {
            status.push_str(&format!(
                " Быстрый доступ ({}) не зарегистрирован — клавиши заняты другой программой.",
                quick_hotkey.label()
            ));
        }
        Self {
            path: startup_path,
            hide_to_tray,
            start_hidden,
            hide_after: None,
            main_hwnd,
            autostart,
            tray_commands: tray::start(main_window_hwnd(cc)),
            allow_exit: false,
            last_target: None,
            pending_browser: None,
            pending_quick: None,
            screen: Screen::Unlock {
                mode: Mode::Open,
                password: String::new(),
                confirm: String::new(),
                hello_pending: None,
                focus_password: false,
            },
            draft: None,
            confirm_delete: None,
            status,
            error: None,
            clipboard: None,
            last_activity: Instant::now(),
            import_open: false,
            import_path: String::new(),
            import_format: ImportFormat::Auto,
            import_result: None,
            theme,
            help_open: false,
            focus_search: false,
            password_open: false,
            password_current: String::new(),
            password_new: String::new(),
            password_confirm: String::new(),
            password_error: None,
            export_open: false,
            export_format: ExportFormat::Json,
            export_path: String::new(),
            export_password: String::new(),
            export_result: None,
            audit_open: false,
            autotype_countdown: None,
            autotype_notice: None,
            hotkey,
            quick_hotkey,
            settings_open: false,
            hotkey_draft: hotkey.label(),
            hotkey_error: None,
            hotkey_capture: false,
            hotkey_focus: false,
            quick_draft: quick_hotkey.label(),
            quick_error: None,
            quick_capture: false,
            quick_access: None,
            quick_prefill: None,
            gen_open: false,
            gen_mode: GenMode::Chars,
            gen_sample_lang: None,
            gen_opts: GeneratorOptions::default(),
            gen_phrase: PassphraseOptions::default(),
            gen_result: String::new(),
            quick_open: false,
            quick_text: String::new(),
            quick_title: String::new(),
            quick_username: String::new(),
            quick_password: String::new(),
            quick_url: String::new(),
            desc_open: false,
            describe_one: None,
            browser_jobs,
            browser_ask: None,
            browser_domains,
            browser_enabled,
        }
    }

    /// Текущие настройки для записи в settings.txt.
    pub(crate) fn settings_snapshot(&self) -> Settings {
        Settings {
            theme: self.theme,
            hotkey: self.hotkey,
            quick_hotkey: self.quick_hotkey,
            vault_path: Some(self.path.clone()),
            browser: self.browser_enabled,
            tray: self.hide_to_tray,
            autostart: self.autostart,
        }
    }

    /// Показывает окно из трея и после запросов браузера.
    pub(crate) fn show_main(&self, ctx: &egui::Context) {
        show_window_now(self.main_hwnd);
        ctx.request_repaint();
    }

    /// Закрытие окна: спрятать в трей или выйти по-настоящему.
    pub(crate) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) || self.allow_exit {
            return;
        }
        if self.hide_to_tray {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            hide_window(self.main_hwnd);
            self.status =
                "«Хранитель» свёрнут в трей — значки горячих клавиш работают.".to_string();
            ctx.request_repaint();
        } else {
            self.allow_exit = true;
        }
    }

    /// Команды из меню значка в трее.
    pub(crate) fn handle_tray_commands(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.tray_commands.take() else {
            return;
        };
        let mut commands = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            commands.push(command);
        }
        self.tray_commands = Some(receiver);

        for command in commands {
            match command {
                tray::TrayCommand::Open => self.show_main(ctx),
                tray::TrayCommand::QuickAccess => {
                    self.show_main(ctx);
                    if self.screen.is_main() {
                        let (hwnd, title) = self.last_target.clone().unwrap_or((0, String::new()));
                        self.quick_access = Some(QuickAccess {
                            query: String::new(),
                            cursor: 0,
                            target_title: (!title.is_empty()).then_some(title),
                            target_hwnd: hwnd,
                            focus: true,
                        });
                        self.status =
                            "Быстрый доступ: начните печатать, Enter — ввести в окно.".to_string();
                    } else {
                        self.pending_quick = Some(autotype::QuickRequest {
                            title: self.last_target.as_ref().map(|(_, title)| title.clone()),
                            hwnd: self
                                .last_target
                                .as_ref()
                                .map(|(hwnd, _)| *hwnd)
                                .unwrap_or(0),
                        });
                        self.screen.set_focus_password(true);
                        self.status =
                            "Быстрый доступ: сначала разблокируйте хранилище.".to_string();
                    }
                }
                tray::TrayCommand::Lock => {
                    if self.screen.is_main() {
                        self.lock("Заблокировано из трея.");
                    }
                    self.show_main(ctx);
                }
                tray::TrayCommand::Settings => {
                    self.show_main(ctx);
                    self.settings_open = true;
                    self.hotkey_draft = self.hotkey.label();
                    self.hotkey_error = None;
                    self.hotkey_capture = false;
                    self.hotkey_focus = true;
                }
                tray::TrayCommand::Exit => {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Запоминает последнее чужое окно на переднем плане.
    pub(crate) fn remember_target_window(&mut self) {
        if let Some(target) = autotype::foreground_target() {
            self.last_target = Some(target);
        }
    }

    pub(crate) fn lock(&mut self, reason: &str) {
        // Висящий запрос браузера/палитры без хранилища исполнить нельзя.
        if let Some((_, reply)) = self.pending_browser.take() {
            let _ = reply.send(browser::error_json(
                "хранилище заблокировано — откройте его и повторите",
            ));
        }
        self.pending_quick = None;
        // Экран работы целиком заменяется экраном входа: хранилище,
        // выбор, поиск и показанные пароли уничтожаются вместе с ним.
        let mut old_screen = std::mem::replace(
            &mut self.screen,
            Screen::Unlock {
                mode: Mode::Open,
                password: String::new(),
                confirm: String::new(),
                hello_pending: None,
                focus_password: false,
            },
        );
        if let Screen::Unlock {
            password, confirm, ..
        } = &mut old_screen
        {
            password.zeroize();
            confirm.zeroize();
        }
        drop(old_screen);
        self.draft = None;
        self.confirm_delete = None;
        self.error = None;
        self.status = reason.to_string();
        self.import_open = false;
        self.import_result = None;
        // Секреты из генератора и быстрой вставки не живут в заблокированном окне.
        self.gen_open = false;
        self.gen_result.zeroize();
        self.quick_open = false;
        self.quick_text.zeroize();
        self.quick_password.zeroize();
        self.desc_open = false;
        // Палитра быстрого доступа тоже не переживает блокировку.
        self.quick_access = None;
        self.quick_prefill = None;
        // Запрос расширения, повисший в диалоге, отвечаем отказом.
        if let Some(ask) = self.browser_ask.take() {
            ask.deny("Хранилище заблокировано — откройте его и повторите вход на сайте.");
        }
    }

    pub(crate) fn copy(&mut self, text: &str, label: &str) {
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

    pub(crate) fn tick(&mut self, ctx: &egui::Context) {
        self.poll_hello_unlock();

        // Нажата клавиша быстрого доступа: поток уже запомнил целевое окно
        // и поднял главное окно — открываем палитру поверх.
        if let Some(request) = autotype::take_quick_request() {
            if self.screen.is_main() {
                self.quick_access = Some(QuickAccess {
                    query: String::new(),
                    cursor: 0,
                    target_title: request.title,
                    target_hwnd: request.hwnd,
                    focus: true,
                });
                // Адрес страницы читает отдельный поток: UI Automation в
                // Chromium может думать до секунды, палитра ждать не должна.
                let (sender, receiver) = std::sync::mpsc::channel();
                let hwnd = request.hwnd;
                std::thread::spawn(move || {
                    let query = uia::address_bar_url(hwnd)
                        .as_deref()
                        .map(App::site_domain)
                        .unwrap_or_default();
                    let _ = sender.send(query);
                });
                self.quick_prefill = Some(receiver);
                self.status =
                    "Быстрый доступ: начните печатать, Enter — ввести в окно.".to_string();
            } else {
                // Заблокировано: запомним запрос и попросим войти — палитра
                // откроется сама после разблокировки.
                self.pending_quick = Some(request);
                self.screen.set_focus_password(true);
                self.show_main(ctx);
                self.status =
                    "Быстрый доступ: сначала введите мастер-пароль — палитра откроется сама."
                        .to_string();
            }
        }

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

        if self.screen.is_main() && self.last_activity.elapsed() >= AUTO_LOCK {
            self.lock("Заблокировано по бездействию. Введите мастер-пароль снова.");
        }

        ctx.request_repaint_after(Duration::from_millis(500));
    }

    pub(crate) fn track_activity(&mut self, ctx: &egui::Context) {
        let active = ctx.input(|i| i.pointer.any_click() || !i.events.is_empty());
        if active {
            self.last_activity = Instant::now();
        }
    }

    pub(crate) fn submit_unlock(&mut self) {
        self.error = None;
        let path = PathBuf::from(self.path.trim());
        // Значения читаются в локальные переменные, чтобы запись результата
        // могла заменить весь экран целиком.
        let (mode, password, confirm) = match &mut self.screen {
            Screen::Unlock {
                mode,
                password,
                confirm,
                ..
            } => (*mode, password.clone(), confirm.clone()),
            // Уже открыто: повторное нажатие ничего не делает.
            Screen::Main { .. } => return,
        };
        let result = match mode {
            Mode::Open => Vault::open(&path, &password),
            Mode::Create => {
                if password != confirm {
                    self.error = Some("Пароли не совпадают.".into());
                    return;
                }
                Vault::create(&path, &password)
            }
        };

        match result {
            Ok(vault) => {
                let first = vault.items_sorted().first().map(|item| item.id);
                self.status = match mode {
                    Mode::Open => "Хранилище открыто.".to_string(),
                    Mode::Create => "Хранилище создано.".to_string(),
                };
                // Запоминаем файл: при следующем запуске поле уже заполнено.
                self.path = vault.path().display().to_string();
                save_settings(&self.settings_snapshot());
                // Экран входа (с паролями) заменяется экраном работы;
                // старые строки занулятся при уничтожении структуры.
                self.screen = Screen::Main {
                    vault: Some(vault),
                    selected: first,
                    query: String::new(),
                    kind_filter: KindGroup::All,
                    reveal_password: false,
                };
                self.last_activity = Instant::now();
                self.error = None;
                // Пока хранилище было закрыто, браузер или палитра могли
                // попросить запись — теперь можно ответить.
                if let Some((request, reply)) = self.pending_browser.take() {
                    self.handle_browser(request, reply);
                }
                if let Some(quick) = self.pending_quick.take() {
                    self.quick_access = Some(QuickAccess {
                        query: String::new(),
                        cursor: 0,
                        target_title: quick.title,
                        target_hwnd: quick.hwnd,
                        focus: true,
                    });
                    self.status =
                        "Быстрый доступ: начните печатать, Enter — ввести в окно.".to_string();
                }
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Отсчёт перед вводом и всплывающее сообщение о результате.
    pub(crate) fn autotype_overlay(&mut self, ctx: &egui::Context) {
        if let Some(deadline) = self.autotype_countdown {
            let now = Instant::now();
            if now >= deadline {
                self.autotype_countdown = None;
            } else {
                let left = (deadline - now).as_secs_f32().ceil() as u64;
                egui::Window::new("Автоввод через…")
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new(format!("Переключитесь в целевое окно… {left}"))
                                .strong()
                                .size(18.0),
                        );
                    });
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        let mut expired = false;
        if let Some((message, at)) = &self.autotype_notice {
            if at.elapsed() < Duration::from_secs(5) {
                egui::Window::new("Автоввод")
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new(message)
                                .color(egui::Color32::from_rgb(190, 110, 40))
                                .strong(),
                        );
                    });
                ctx.request_repaint_after(Duration::from_millis(200));
            } else {
                expired = true;
            }
        }
        if expired {
            self.autotype_notice = None;
        }
    }

    /// Состояние автоввода: закрыто, без выбора или готовая запись.
    pub(crate) fn autotype_state(&self) -> autotype::AutotypeState {
        let Some(vault) = self.screen.vault() else {
            return autotype::AutotypeState::Locked;
        };
        let Some(item) = self.screen.selected().and_then(|id| vault.get(id)) else {
            return autotype::AutotypeState::NoSelection;
        };
        autotype::AutotypeState::Ready(autotype::Credentials {
            title: item.title.clone(),
            username: item.username.clone(),
            password: Zeroizing::new(item.password.clone()),
        })
    }

    /// Название записи, готовой к автовводу (для подсказки в углу).
    pub(crate) fn autotype_title(&self) -> Option<String> {
        let vault = self.screen.vault()?;
        let item = vault.get(self.screen.selected()?)?;
        Some(item.title.clone())
    }

    /// Сайдкар Windows Hello для выбранного пути, если он существует.
    pub(crate) fn hello_sidecar(&self) -> Option<PathBuf> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let sidecar = sidecar_path(Path::new(path));
        sidecar.exists().then_some(sidecar)
    }

    /// Запускает вход через Hello в фоне: DPAPI, подтверждение и расшифровка
    /// хранилища не должны подвешивать отрисовку.
    pub(crate) fn start_hello_unlock(&mut self, ctx: &egui::Context) {
        let path = PathBuf::from(self.path.trim());
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            let result = (|| -> Result<Vault, String> {
                // Сначала проверяем, что сайдкар читается, затем спрашиваем Hello
                // и только после подтверждения снимаем защиту с ключа.
                let blob = protected_wrapping_key(&path).map_err(|err| err.to_string())?;
                hello_win::request_consent()?;
                let wrapping = hello_win::dpapi_unprotect(&blob)?;
                Vault::open_with_hello(&path, &wrapping).map_err(|err| err.to_string())
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });

        self.screen.set_hello_pending(Some(receiver));
        self.error = None;
        self.status = "Ожидание подтверждения Windows Hello…".to_string();
    }

    /// Забирает результат фонового входа через Hello.
    pub(crate) fn poll_hello_unlock(&mut self) {
        let Some(receiver) = self.screen.take_hello_pending() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(vault)) => {
                let first = vault.items_sorted().first().map(|item| item.id);
                // Экран входа заменяется экраном работы.
                self.screen = Screen::Main {
                    vault: Some(vault),
                    selected: first,
                    query: String::new(),
                    kind_filter: KindGroup::All,
                    reveal_password: false,
                };
                self.error = None;
                self.last_activity = Instant::now();
                self.status = "Хранилище открыто через Windows Hello.".to_string();
                // Отложенные запросы браузера и палитры теперь можно выполнить.
                if let Some((request, reply)) = self.pending_browser.take() {
                    self.handle_browser(request, reply);
                }
                if let Some(quick) = self.pending_quick.take() {
                    self.quick_access = Some(QuickAccess {
                        query: String::new(),
                        cursor: 0,
                        target_title: quick.title,
                        target_hwnd: quick.hwnd,
                        focus: true,
                    });
                }
            }
            Ok(Err(err)) => {
                self.error = Some(format!("{err}. Введите мастер-пароль."));
                self.status = "Вход через Windows Hello не удался.".to_string();
            }
            // Ещё думает: вернуть канал на место и спросить в следующем кадре.
            Err(TryRecvError::Empty) => self.screen.set_hello_pending(Some(receiver)),
            Err(TryRecvError::Disconnected) => {
                self.error = Some(
                    "Поток Windows Hello завершился неожиданно. Введите мастер-пароль.".to_string(),
                );
            }
        }
    }

    /// Домен сайта по адресу запроса (для разрешения доступа).
    pub(crate) fn site_domain(url: &str) -> String {
        match normalize_host(url) {
            Some(host) => registrable_domain(&host),
            None => String::new(),
        }
    }
}

/// Образец фразы устарел, если он составлен для другого языка.
/// После `regenerate()` отметка совпадает с языком, и образец не пересоздаётся.
/// Сравнивать сам текст нельзя: проверка «ascii или нет» всегда была истинной,
/// и фраза пересоздавалась на каждом кадре — выглядело как бесконечный перебор.
pub(crate) fn phrase_sample_stale(
    mode: GenMode,
    sample_lang: Option<Wordlist>,
    language: Wordlist,
) -> bool {
    mode == GenMode::Phrase && sample_lang != Some(language)
}

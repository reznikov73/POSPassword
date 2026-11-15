//! Оконный клиент «Хранитель» на eframe/egui.
//!
//! Вся криптография и работа с файлом — в `vault-core`; здесь только UI,
//! буфер обмена с автоочисткой и автоблокировка.

// В релизе не открываем консольное окно рядом с интерфейсом: оно забирает
// фокус и клавиши (включая F1). В отладочной сборке консоль остаётся для логов.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autotype;
mod filedialog;
mod hello_win;
mod help;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use uuid::Uuid;
use vault_core::{
    ExportFormat, Field, GeneratorOptions, ImportFormat, Item, ItemKind, MatchStrength,
    PassphraseOptions, Separator, Totp, Vault, Wordlist, auto_description, expiries,
    generate_passphrase, generate_password, generate_wrapping_key, import_file, now_secs,
    parse_credentials, passphrase_entropy_bits, password_entropy_bits, protected_wrapping_key,
    sidecar_path,
};
use zeroize::{Zeroize, Zeroizing};

const APP_TITLE: &str = "Хранитель";
const AUTO_LOCK: Duration = Duration::from_secs(5 * 60);
const CLIPBOARD_TTL: Duration = Duration::from_secs(30);

fn main() -> eframe::Result {
    // Один экземпляр: два окна путают автоввод — горячую клавишу держит только одно.
    if other_instance_running() {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
            use windows::core::w;
            MessageBoxW(
                None,
                w!(
                    "«Хранитель» уже запущен в другом окне.\n\nЗакройте старое окно и запустите это снова — иначе автоввод будет работать из старого экземпляра."
                ),
                w!("Хранитель"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        std::process::exit(0);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1020.0, 680.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title(format!("{APP_TITLE} {}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

/// Хэндл мьютекса текущего экземпляра. Нужен только чтобы не закрывать его.
static INSTANCE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Пытается взять именованный мьютекс. `true` — другой экземпляр уже работает.
fn other_instance_running() -> bool {
    use windows::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, GetLastError, SetLastError, WIN32_ERROR,
    };
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;

    // SAFETY: имя — статическая строка, атрибуты не нужны.
    unsafe {
        // Код последней ошибки может быть сталым от прошлых вызовов.
        SetLastError(WIN32_ERROR(0));
        match CreateMutexW(None, true, w!("Hranitel.SingleInstance")) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    true
                } else {
                    // Хэндл нигде не закрываем: он живёт до конца процесса
                    // и держит мьютекс всё время работы.
                    INSTANCE.store(handle.0 as isize, std::sync::atomic::Ordering::Relaxed);
                    false
                }
            }
            Err(_) => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Open,
    Create,
}

/// Оформление окна. Выбор сохраняется между запусками.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Theme {
    Light,
    Dark,
}

/// Режим генератора паролей: случайные символы или запоминаемая фраза.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GenMode {
    Chars,
    Phrase,
}

impl Theme {
    fn label(self) -> &'static str {
        match self {
            Theme::Light => "светлая",
            Theme::Dark => "тёмная",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    fn apply(self, ctx: &egui::Context) {
        // egui хранит отдельные стили для светлой и тёмной темы; задаём оба,
        // а затем жёстко выбираем нужную, чтобы системная тема не перебивала.
        ctx.set_visuals_of(egui::Theme::Light, theme_visuals(true));
        ctx.set_visuals_of(egui::Theme::Dark, theme_visuals(false));
        match self {
            Theme::Light => ctx.set_theme(egui::ThemePreference::Light),
            Theme::Dark => ctx.set_theme(egui::ThemePreference::Dark),
        }
    }
}

fn theme_visuals(light: bool) -> egui::Visuals {
    let mut visuals = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    // Контрастная тема: почти чёрный текст, заметные рамки у полей и кнопок.
    if light {
        let text = egui::Color32::from_rgb(16, 18, 22);
        let border = egui::Color32::from_rgb(118, 128, 143);
        let accent = egui::Color32::from_rgb(38, 92, 180);
        let panel = egui::Color32::from_rgb(236, 239, 244);

        visuals.override_text_color = Some(text);
        visuals.panel_fill = panel;
        visuals.window_fill = egui::Color32::WHITE;
        visuals.faint_bg_color = egui::Color32::from_rgb(226, 231, 238);
        visuals.extreme_bg_color = egui::Color32::WHITE;
        visuals.window_stroke = egui::Stroke::new(1.0, border);
        visuals.window_corner_radius = egui::CornerRadius::same(10);
        visuals.selection.bg_fill = accent;
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

        visuals.widgets.noninteractive.bg_fill = panel;
        visuals.widgets.noninteractive.weak_bg_fill = panel;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);

        visuals.widgets.inactive.bg_fill = egui::Color32::WHITE;
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::WHITE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(219, 231, 250);
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(228, 238, 252);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, accent);

        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(200, 218, 245);
        visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(210, 226, 248);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, accent);

        visuals.widgets.open.bg_fill = egui::Color32::WHITE;
        visuals.widgets.open.weak_bg_fill = egui::Color32::WHITE;
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, text);
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, accent);
    } else {
        let text = egui::Color32::from_rgb(235, 238, 244);
        let border = egui::Color32::from_rgb(96, 106, 122);
        let accent = egui::Color32::from_rgb(104, 156, 240);
        let panel = egui::Color32::from_rgb(27, 30, 36);

        visuals.override_text_color = Some(text);
        visuals.panel_fill = panel;
        visuals.window_fill = egui::Color32::from_rgb(33, 37, 44);
        visuals.faint_bg_color = egui::Color32::from_rgb(40, 44, 52);
        visuals.extreme_bg_color = egui::Color32::from_rgb(19, 22, 27);
        visuals.window_stroke = egui::Stroke::new(1.0, border);
        visuals.window_corner_radius = egui::CornerRadius::same(10);
        visuals.selection.bg_fill = egui::Color32::from_rgb(52, 92, 156);
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

        visuals.widgets.noninteractive.bg_fill = panel;
        visuals.widgets.noninteractive.weak_bg_fill = panel;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);

        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(42, 47, 56);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(42, 47, 56);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(54, 62, 76);
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(54, 62, 76);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, accent);

        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(62, 74, 92);
        visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(62, 74, 92);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, accent);

        visuals.widgets.open.bg_fill = egui::Color32::from_rgb(42, 47, 56);
        visuals.widgets.open.weak_bg_fill = egui::Color32::from_rgb(42, 47, 56);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, text);
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, accent);
    }
    visuals
}

fn settings_path() -> PathBuf {
    app_dir().join("settings.txt")
}

/// Настройки программы: тема, сочетания клавиш и последний путь к хранилищу.
#[derive(Clone)]
struct Settings {
    theme: Theme,
    hotkey: autotype::Hotkey,
    quick_hotkey: autotype::Hotkey,
    vault_path: Option<String>,
}

fn load_settings() -> Settings {
    let mut settings = Settings {
        theme: Theme::Light,
        hotkey: autotype::Hotkey::default(),
        quick_hotkey: autotype::quick_default(),
        vault_path: None,
    };
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return settings;
    };
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line == "dark" {
            settings.theme = Theme::Dark;
            continue;
        }
        if line == "light" {
            settings.theme = Theme::Light;
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        match name.trim() {
            "theme" => {
                settings.theme = if value.trim() == "dark" {
                    Theme::Dark
                } else {
                    Theme::Light
                };
            }
            "hotkey" => {
                if let Ok(hotkey) = autotype::Hotkey::parse(value.trim()) {
                    settings.hotkey = hotkey;
                }
            }
            "quickkey" => {
                if let Ok(hotkey) = autotype::Hotkey::parse(value.trim()) {
                    settings.quick_hotkey = hotkey;
                }
            }
            "path" => {
                let path = value.trim();
                if !path.is_empty() {
                    settings.vault_path = Some(path.to_string());
                }
            }
            _ => {}
        }
    }
    settings
}

fn save_settings(
    theme: Theme,
    hotkey: autotype::Hotkey,
    quick: autotype::Hotkey,
    vault_path: &str,
) {
    let dir = app_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let theme_value = match theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    let mut text = format!(
        "theme={theme_value}\nhotkey={}\nquickkey={}\n",
        hotkey.label().to_lowercase(),
        quick.label().to_lowercase()
    );
    // Последний открытый файл: при следующем запуске поле уже заполнено.
    let path = vault_path.trim();
    if !path.is_empty() {
        text.push_str(&format!("path={path}\n"));
    }
    let _ = std::fs::write(dir.join("settings.txt"), text);
}

/// Категория для фильтра списка.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KindGroup {
    All,
    Logins,
    Cards,
    Accounts,
    Notes,
    Documents,
    Keys,
}

impl KindGroup {
    fn all() -> [KindGroup; 7] {
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

    fn label(self) -> &'static str {
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

    fn matches(self, kind: ItemKind) -> bool {
        self == KindGroup::All || group_of(kind) == self
    }
}

fn group_of(kind: ItemKind) -> KindGroup {
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

struct Draft {
    id: Option<Uuid>,
    kind: ItemKind,
    title: String,
    username: String,
    password: String,
    url: String,
    /// Дополнительные сайты записи — по одному адресу в строке.
    urls: String,
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
            urls: String::new(),
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
            urls: item.urls.join("\n"),
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
fn template_fields(kind: ItemKind) -> Vec<Field> {
    kind.template()
}

enum DraftOutcome {
    Idle,
    Cancel,
    Save { item: Box<Item>, is_new: bool },
}

/// Состояние палитры быстрого доступа: фильтр, курсор и целевое окно,
/// из которого палитра вызвана.
struct QuickAccess {
    query: String,
    /// Выбранная строка списка.
    cursor: usize,
    /// Заголовок целевого окна — по нему сортируются записи.
    target_title: Option<String>,
    /// Дескриптор целевого окна для возврата фокуса (0 — неизвестно).
    target_hwnd: isize,
    /// Поставить курсор в фильтр при открытии.
    focus: bool,
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
    theme: Theme,
    /// Фоновый запрос Windows Hello; результат забирается в `tick`.
    hello_pending: Option<Receiver<Result<Vault, String>>>,
    /// Окно встроенной справки, открывается по F1.
    help_open: bool,
    /// Курсор должен прыгнуть в поиск на следующем кадре (Ctrl+F).
    focus_search: bool,
    /// Диалог смены мастер-пароля.
    password_open: bool,
    password_current: String,
    password_new: String,
    password_confirm: String,
    password_error: Option<String>,
    /// Диалог экспорта.
    export_open: bool,
    export_format: ExportFormat,
    export_path: String,
    export_password: String,
    export_result: Option<String>,
    /// Панель аудита хранилища.
    audit_open: bool,
    /// Сочетание клавиш автоввода.
    hotkey: autotype::Hotkey,
    /// Сочетание клавиш быстрого доступа.
    quick_hotkey: autotype::Hotkey,
    /// Диалог настроек.
    settings_open: bool,
    hotkey_draft: String,
    hotkey_error: Option<String>,
    /// Идёт запись нового сочетания: следующее нажатие станет горячей клавишей.
    hotkey_capture: bool,
    /// Поставить курсор в поле сочетания при открытии настроек.
    hotkey_focus: bool,
    /// Те же поля для сочетания быстрого доступа.
    quick_draft: String,
    quick_error: Option<String>,
    quick_capture: bool,
    /// Палитра быстрого доступа, открыта пока поле не None.
    quick_access: Option<QuickAccess>,
    /// Момент, когда надо выполнить автоввод после кнопки с отсчётом.
    autotype_countdown: Option<Instant>,
    /// Сообщение автоввода на несколько секунд поверх окна.
    autotype_notice: Option<(String, Instant)>,
    /// Фильтр списка по категории.
    kind_filter: KindGroup,
    /// Диалог генератора паролей.
    gen_open: bool,
    gen_mode: GenMode,
    /// Язык, для которого сейчас составлен образец фразы: по нему видно,
    /// что образец устарел после смены языка (сравнивать текст нельзя —
    /// после смены языка образец пересоздавался каждый кадр).
    gen_sample_lang: Option<Wordlist>,
    gen_opts: GeneratorOptions,
    gen_phrase: PassphraseOptions,
    gen_result: String,
    /// Диалог быстрой вставки логина и пароля.
    quick_open: bool,
    quick_text: String,
    /// Поля быстрой вставки — редактируются после разбора.
    quick_title: String,
    quick_username: String,
    quick_password: String,
    quick_url: String,
    /// Диалог массового добавления описаний.
    desc_open: bool,
    /// Запрос на описание одной записи из карточки.
    describe_one: Option<Uuid>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = load_settings();
        let theme = settings.theme;
        let hotkey = settings.hotkey;
        let quick_hotkey = settings.quick_hotkey;
        let startup_path = settings
            .vault_path
            .clone()
            .unwrap_or_else(default_vault_path);
        theme.apply(&cc.egui_ctx);
        install_fonts(&cc.egui_ctx);

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
            mode: Mode::Open,
            path: startup_path,
            password: String::new(),
            confirm: String::new(),
            vault: None,
            query: String::new(),
            selected: None,
            draft: None,
            confirm_delete: None,
            status,
            error: None,
            reveal_password: false,
            clipboard: None,
            last_activity: Instant::now(),
            import_open: false,
            import_path: String::new(),
            import_format: ImportFormat::Auto,
            import_result: None,
            theme,
            hello_pending: None,
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
            kind_filter: KindGroup::All,
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
        self.hello_pending = None;
        // Секреты из генератора и быстрой вставки не живут в заблокированном окне.
        self.gen_open = false;
        self.gen_result.zeroize();
        self.quick_open = false;
        self.quick_text.zeroize();
        self.quick_password.zeroize();
        self.desc_open = false;
        // Палитра быстрого доступа тоже не переживает блокировку.
        self.quick_access = None;
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
        self.poll_hello_unlock();

        // Нажата клавиша быстрого доступа: поток уже запомнил целевое окно
        // и поднял главное окно — открываем палитру поверх.
        if let Some(request) = autotype::take_quick_request() {
            if self.vault.is_some() {
                self.quick_access = Some(QuickAccess {
                    query: String::new(),
                    cursor: 0,
                    target_title: request.title,
                    target_hwnd: request.hwnd,
                    focus: true,
                });
                self.status =
                    "Быстрый доступ: начните печатать, Enter — ввести в окно.".to_string();
            } else {
                self.status =
                    "Хранилище заблокировано — сначала введите мастер-пароль.".to_string();
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
                let first = vault.items_sorted().first().map(|item| item.id);
                self.status = match self.mode {
                    Mode::Open => "Хранилище открыто.".to_string(),
                    Mode::Create => "Хранилище создано.".to_string(),
                };
                // Запоминаем файл: при следующем запуске поле уже заполнено.
                self.path = vault.path().display().to_string();
                save_settings(self.theme, self.hotkey, self.quick_hotkey, &self.path);
                self.vault = Some(vault);
                self.selected = first;
                self.hello_pending = None;
                self.password.zeroize();
                self.confirm.zeroize();
                self.mode = Mode::Open;
                self.last_activity = Instant::now();
                self.error = None;
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Отсчёт перед вводом и всплывающее сообщение о результате.
    fn autotype_overlay(&mut self, ctx: &egui::Context) {
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
    fn autotype_state(&self) -> autotype::AutotypeState {
        let Some(vault) = self.vault.as_ref() else {
            return autotype::AutotypeState::Locked;
        };
        let Some(item) = self.selected.and_then(|id| vault.get(id)) else {
            return autotype::AutotypeState::NoSelection;
        };
        autotype::AutotypeState::Ready(autotype::Credentials {
            title: item.title.clone(),
            username: item.username.clone(),
            password: Zeroizing::new(item.password.clone()),
        })
    }

    /// Название записи, готовой к автовводу (для подсказки в углу).
    fn autotype_title(&self) -> Option<String> {
        let vault = self.vault.as_ref()?;
        let item = vault.get(self.selected?)?;
        Some(item.title.clone())
    }

    /// Сайдкар Windows Hello для выбранного пути, если он существует.
    fn hello_sidecar(&self) -> Option<PathBuf> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let sidecar = sidecar_path(Path::new(path));
        sidecar.exists().then_some(sidecar)
    }

    /// Запускает вход через Hello в фоне: DPAPI, подтверждение и расшифровка
    /// хранилища не должны подвешивать отрисовку.
    fn start_hello_unlock(&mut self, ctx: &egui::Context) {
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

        self.hello_pending = Some(receiver);
        self.error = None;
        self.status = "Ожидание подтверждения Windows Hello…".to_string();
    }

    /// Забирает результат фонового входа через Hello.
    fn poll_hello_unlock(&mut self) {
        let Some(receiver) = &self.hello_pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(vault)) => {
                self.hello_pending = None;
                let first = vault.items_sorted().first().map(|item| item.id);
                self.vault = Some(vault);
                self.selected = first;
                self.password.zeroize();
                self.confirm.zeroize();
                self.mode = Mode::Open;
                self.error = None;
                self.last_activity = Instant::now();
                self.status = "Хранилище открыто через Windows Hello.".to_string();
            }
            Ok(Err(err)) => {
                self.hello_pending = None;
                self.error = Some(format!("{err}. Введите мастер-пароль."));
                self.status = "Вход через Windows Hello не удался.".to_string();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.hello_pending = None;
                self.error = Some(
                    "Поток Windows Hello завершился неожиданно. Введите мастер-пароль.".to_string(),
                );
            }
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

                if self.hello_pending.is_some() {
                    ui.add_space(8.0);
                    ui.label("Ожидание подтверждения Windows Hello…");
                    ui.ctx().request_repaint_after(Duration::from_millis(200));
                } else if self.hello_sidecar().is_some() {
                    ui.add_space(8.0);
                    if ui
                        .button("Войти через Windows Hello")
                        .on_hover_text("Разблокировка без мастер-пароля")
                        .clicked()
                    {
                        self.start_hello_unlock(ui.ctx());
                    }
                }

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
            ui.add_space(8.0);
            if ui
                .small_button(format!("Тема: {}", self.theme.label()))
                .clicked()
            {
                self.theme = self.theme.toggled();
                self.theme.apply(ui.ctx());
                save_settings(self.theme, self.hotkey, self.quick_hotkey, &self.path);
            }
            if ui.small_button("Справка (F1)").clicked() {
                self.help_open = true;
            }
            if ui.small_button("Настройки").clicked() {
                self.settings_open = true;
                self.hotkey_draft = self.hotkey.label();
                self.hotkey_error = None;
                self.hotkey_capture = false;
                self.hotkey_focus = true;
            }
        });
    }

    fn main_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut vault) = self.vault.take() else {
            return;
        };
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
            && let Some(id) = self.selected
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
            if ui.button("Экспорт").on_hover_text("Копия в JSON или CSV").clicked() {
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
            let hello_label = if hello_on { "Hello: вкл" } else { "Hello: выкл" };
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
                } else {
                    let wrapping = generate_wrapping_key();
                    match hello_win::dpapi_protect(&wrapping) {
                        Ok(blob) => match vault.enable_hello(&wrapping, &blob) {
                            Ok(()) => {
                                self.status = "Вход через Windows Hello включён. При следующем запуске мастер-пароль не понадобится.".to_string();
                            }
                            Err(err) => {
                                self.status = format!("Не удалось включить Hello: {err}");
                            }
                        },
                        Err(err) => {
                            self.status = format!("Не удалось защитить ключ: {err}");
                        }
                    }
                }
            }
            if ui.button(format!("Тема: {}", self.theme.label())).clicked() {
                self.theme = self.theme.toggled();
                self.theme.apply(ui.ctx());
                save_settings(self.theme, self.hotkey, self.quick_hotkey, &self.path);
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
                    ui.horizontal(|ui| {
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
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
                                    self.kind_filter == group,
                                    egui::RichText::new(label).small(),
                                )
                                .clicked()
                            {
                                self.kind_filter = group;
                            }
                        }
                    });
                    ui.add_space(6.0);

                    let items: Vec<(Uuid, String, String, ItemKind, String)> = vault
                        .search(&self.query)
                        .into_iter()
                        .filter(|item| self.kind_filter.matches(item.kind))
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
                        self.selected = Some(item.id);
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
            self.vault = Some(vault);
        }
    }

    /// Диалог настроек: сочетание клавиш автоввода.
    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let mut open = true;
        let mut save = false;
        let mut cancel = false;

        egui::Window::new("Настройки")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Сочетание клавиш для автоввода");
                ui.horizontal(|ui| {
                    if self.hotkey_capture {
                        ui.label(
                            egui::RichText::new("Нажмите новое сочетание…")
                                .strong()
                                .size(16.0),
                        );
                    } else {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.hotkey_draft)
                                .hint_text("Ctrl+Alt+P")
                                .desired_width(220.0),
                        );
                        if self.hotkey_focus {
                            edit.request_focus();
                            self.hotkey_focus = false;
                        }
                    }
                    let label = if self.hotkey_capture {
                        "Отменить запись"
                    } else {
                        "Записать сочетание"
                    };
                    if ui.button(label).clicked() {
                        self.hotkey_capture = !self.hotkey_capture;
                        self.hotkey_error = None;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Можно просто нажать сочетание целиком или вписать его текстом: Ctrl+Alt+P, Ctrl+Shift+F2. Нужен хотя бы один модификатор.",
                    )
                    .weak()
                    .small(),
                );
                ui.label(
                    egui::RichText::new(format!("Сейчас: {}", self.hotkey.label()))
                        .small()
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(if autotype::is_ready() {
                        "Клавиша зарегистрирована и работает."
                    } else {
                        "Клавиша сейчас не работает — комбинацию заняла другая программа."
                    })
                    .small()
                    .weak(),
                );

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label("Сочетание клавиш быстрого доступа");
                ui.horizontal(|ui| {
                    if self.quick_capture {
                        ui.label(
                            egui::RichText::new("Нажмите новое сочетание…")
                                .strong()
                                .size(16.0),
                        );
                    } else {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_draft)
                                .hint_text("Ctrl+Shift+Space")
                                .desired_width(220.0),
                        );
                    }
                    let label = if self.quick_capture {
                        "Отменить запись"
                    } else {
                        "Записать сочетание"
                    };
                    if ui.button(label).clicked() {
                        self.quick_capture = !self.quick_capture;
                        self.quick_error = None;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Нажатая комбинация из любого окна поднимает палитру выбора записи: фильтруйте, Enter — ввести логин и пароль в прежнее окно.",
                    )
                    .weak()
                    .small(),
                );
                ui.label(
                    egui::RichText::new(format!("Сейчас: {}", self.quick_hotkey.label()))
                        .small()
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(if autotype::is_quick_ready() {
                        "Клавиша быстрого доступа зарегистрирована и работает."
                    } else {
                        "Клавиша быстрого доступа сейчас не работает — комбинацию заняла другая программа."
                    })
                    .small()
                    .weak(),
                );

                if let Some(error) = &self.quick_error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(200, 60, 60), error);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() {
                        save = true;
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                });
            });

        // Пока идёт запись, следующая нажатая клавиша становится сочетанием.
        if self.hotkey_capture {
            let pressed = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, modifiers)) = pressed {
                if key == egui::Key::Escape {
                    self.hotkey_capture = false;
                } else if let Some(hotkey) = autotype::Hotkey::from_egui(key, modifiers) {
                    self.hotkey_capture = false;
                    self.hotkey_draft = hotkey.label();
                    self.apply_hotkey(hotkey);
                } else {
                    self.hotkey_error = Some(
                        "нужны модификатор (Ctrl, Alt или Shift) и буква, цифра либо F1–F24"
                            .to_string(),
                    );
                }
            }
            ctx.request_repaint();
        }

        // То же для сочетания быстрого доступа.
        if self.quick_capture {
            let pressed = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, modifiers)) = pressed {
                if key == egui::Key::Escape {
                    self.quick_capture = false;
                } else if let Some(hotkey) = autotype::Hotkey::from_egui(key, modifiers) {
                    self.quick_capture = false;
                    self.quick_draft = hotkey.label();
                    self.apply_quick_hotkey(hotkey);
                } else {
                    self.quick_error = Some(
                        "нужны модификатор (Ctrl, Alt или Shift) и буква, цифра либо F1–F24"
                            .to_string(),
                    );
                }
            }
            ctx.request_repaint();
        }

        if cancel || !open {
            self.settings_open = false;
            self.hotkey_capture = false;
            self.hotkey_error = None;
            self.quick_capture = false;
            self.quick_error = None;
            return;
        }

        if save {
            match autotype::Hotkey::parse(&self.hotkey_draft) {
                Err(err) => self.hotkey_error = Some(err),
                Ok(hotkey) => self.apply_hotkey(hotkey),
            }
            match autotype::Hotkey::parse(&self.quick_draft) {
                Err(err) => self.quick_error = Some(err),
                Ok(hotkey) => self.apply_quick_hotkey(hotkey),
            }
        }
    }

    /// Ставит новое сочетание. Если оно занято, остаётся старое,
    /// а ошибка показывается в диалоге — молча ничего не теряем.
    fn apply_hotkey(&mut self, hotkey: autotype::Hotkey) {
        match autotype::rebind(hotkey) {
            Ok(()) => {
                self.hotkey = hotkey;
                self.hotkey_draft = hotkey.label();
                self.hotkey_error = None;
                self.settings_open = false;
                save_settings(self.theme, self.hotkey, self.quick_hotkey, &self.path);
                self.status = format!("Горячая клавиша автоввода: {}", hotkey.label());
            }
            Err(err) => self.hotkey_error = Some(err),
        }
    }

    /// То же для сочетания быстрого доступа.
    fn apply_quick_hotkey(&mut self, hotkey: autotype::Hotkey) {
        match autotype::rebind_quick(hotkey) {
            Ok(()) => {
                self.quick_hotkey = hotkey;
                self.quick_draft = hotkey.label();
                self.quick_error = None;
                save_settings(self.theme, self.hotkey, self.quick_hotkey, &self.path);
                self.status = format!("Клавиша быстрого доступа: {}", hotkey.label());
            }
            Err(err) => self.quick_error = Some(err),
        }
    }

    /// Диалог смены мастер-пароля.
    fn password_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if !self.password_open {
            return;
        }

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Сменить мастер-пароль")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Текущий пароль");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password_current)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.label("Новый пароль (не короче 8 символов)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password_new)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.label("Повторите новый пароль");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password_confirm)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );

                if let Some(error) = &self.password_error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Сменить").clicked() {
                        submit = true;
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Файл будет перешифрован новым ключом, вход через Windows Hello отключится.",
                    )
                    .weak()
                    .small(),
                );
            });

        if cancel || !open {
            self.password_open = false;
            self.password_current.zeroize();
            self.password_new.zeroize();
            self.password_confirm.zeroize();
            self.password_error = None;
            return;
        }

        if submit {
            self.password_error = None;
            if self.password_new != self.password_confirm {
                self.password_error = Some("Новые пароли не совпадают.".into());
            } else if self.password_new.chars().count() < 8 {
                self.password_error = Some("Новый пароль короче 8 символов.".into());
            } else {
                // Текущий пароль проверяем настоящей расшифровкой файла.
                match Vault::open(vault.path(), &self.password_current) {
                    Err(_) => {
                        self.password_error = Some("Текущий пароль неверен.".into());
                    }
                    Ok(probe) => {
                        drop(probe);
                        match vault.change_password(&self.password_new) {
                            Ok(()) => {
                                self.status = "Мастер-пароль изменён. Вход через Windows Hello отключён — включите заново.".to_string();
                                self.password_open = false;
                                self.password_current.zeroize();
                                self.password_new.zeroize();
                                self.password_confirm.zeroize();
                            }
                            Err(err) => {
                                self.password_error =
                                    Some(format!("Не удалось сменить пароль: {err}"));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Диалог экспорта копии хранилища.
    fn export_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
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
            match Vault::open(vault.path(), &self.export_password) {
                Err(_) => self.export_result = Some("Мастер-пароль неверен.".to_string()),
                Ok(probe) => {
                    drop(probe);
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
                                self.export_result =
                                    Some(format!("Не удалось записать файл: {err}"));
                            }
                        },
                        Err(err) => self.export_result = Some(format!("Ошибка экспорта: {err}")),
                    }
                }
            }
        }
    }

    /// Панель аудита: сроки действия, повторяющиеся и короткие пароли.
    fn audit_dialog(&mut self, ctx: &egui::Context, vault: &Vault) {
        if !self.audit_open {
            return;
        }

        let report = vault.audit();
        let mut open = true;

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
                        draw_findings(
                            ui,
                            "Сроки действия",
                            &report.expiring,
                            &mut self.selected,
                            &mut self.audit_open,
                        );
                        draw_findings(
                            ui,
                            "Повторяющиеся пароли",
                            &report.reused,
                            &mut self.selected,
                            &mut self.audit_open,
                        );
                        draw_findings(
                            ui,
                            "Короткие пароли",
                            &report.weak,
                            &mut self.selected,
                            &mut self.audit_open,
                        );
                        draw_findings(
                            ui,
                            "Давно не обновлялись",
                            &report.old,
                            &mut self.selected,
                            &mut self.audit_open,
                        );
                    });
            });

        if !open {
            self.audit_open = false;
        }
    }

    /// Диалог генератора паролей: случайные символы или фраза из слов.
    fn generator_dialog(&mut self, ctx: &egui::Context) {
        if !self.gen_open {
            return;
        }

        let mut open = true;
        let mut generate = false;
        let opts_before = self.gen_opts;
        let phrase_before = self.gen_phrase;
        let mode_before = self.gen_mode;

        egui::Window::new("Генератор паролей")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.gen_mode, GenMode::Chars, "Случайные символы");
                    ui.selectable_value(&mut self.gen_mode, GenMode::Phrase, "Запоминаемая фраза");
                });
                ui.separator();

                let bits = match self.gen_mode {
                    GenMode::Chars => {
                        ui.horizontal(|ui| {
                            ui.label("Длина");
                            ui.add(
                                egui::Slider::new(&mut self.gen_opts.length, 8..=64)
                                    .text("символов"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.gen_opts.lowercase, "строчные");
                            ui.checkbox(&mut self.gen_opts.uppercase, "заглавные");
                            ui.checkbox(&mut self.gen_opts.digits, "цифры");
                            ui.checkbox(&mut self.gen_opts.symbols, "символы");
                        });
                        ui.checkbox(
                            &mut self.gen_opts.avoid_ambiguous,
                            "без похожих символов (Il1O0)",
                        );
                        self.gen_opts.entropy_bits()
                    }
                    GenMode::Phrase => {
                        ui.horizontal(|ui| {
                            ui.label("Язык слов");
                            egui::ComboBox::from_id_salt("gen_lang")
                                .selected_text(self.gen_phrase.language.label())
                                .show_ui(ui, |ui| {
                                    for lang in Wordlist::all() {
                                        ui.selectable_value(
                                            &mut self.gen_phrase.language,
                                            lang,
                                            lang.label(),
                                        );
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Слов во фразе");
                            ui.add(
                                egui::Slider::new(&mut self.gen_phrase.words, 3..=8)
                                    .text("шт."),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Цифр в конце");
                            ui.add(
                                egui::Slider::new(&mut self.gen_phrase.digits, 0..=4)
                                    .text("шт."),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Разделитель");
                            egui::ComboBox::from_id_salt("gen_sep")
                                .selected_text(self.gen_phrase.separator.label())
                                .show_ui(ui, |ui| {
                                    for sep in Separator::all() {
                                        ui.selectable_value(
                                            &mut self.gen_phrase.separator,
                                            sep,
                                            sep.label(),
                                        );
                                    }
                                });
                            ui.checkbox(&mut self.gen_phrase.capitalize, "с заглавной буквы");
                        });
                        passphrase_entropy_bits(&self.gen_phrase)
                    }
                };

                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.gen_result)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "≈ {bits:.0} бит · {}",
                        strength_word(bits)
                    ))
                    .small()
                    .color(strength_color(bits)),
                );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Сгенерировать").clicked() {
                        generate = true;
                    }
                    if ui
                        .button("Копировать")
                        .on_hover_text("Буфер очистится через 30 секунд")
                        .clicked()
                    {
                        let result = self.gen_result.clone();
                        self.copy(&result, "Пароль");
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Фразу из слов проще запомнить и набрать, чем случайные символы, а стойкость при достаточном числе слов та же.",
                    )
                    .weak()
                    .small(),
                );
            });

        if !open {
            self.gen_open = false;
            self.gen_result.zeroize();
            return;
        }
        // Настройки поменяли — сразу показываем свежий образец. Смену языка
        // отслеживаем по отметке, а не по виду строки: иначе образец
        // пересоздавался на каждом кадре и мелькал без остановки.
        let language_changed = phrase_sample_stale(
            self.gen_mode,
            self.gen_sample_lang,
            self.gen_phrase.language,
        );
        if self.gen_opts != opts_before
            || self.gen_phrase != phrase_before
            || self.gen_mode != mode_before
            || language_changed
            || generate
        {
            self.regenerate();
        }
    }

    /// Генерирует образец по текущим настройкам генератора.
    fn regenerate(&mut self) {
        let result = match self.gen_mode {
            GenMode::Chars => generate_password(&self.gen_opts),
            GenMode::Phrase => generate_passphrase(&self.gen_phrase),
        };
        match result {
            Ok(password) => {
                self.gen_sample_lang = match self.gen_mode {
                    GenMode::Phrase => Some(self.gen_phrase.language),
                    GenMode::Chars => None,
                };
                self.gen_result = password;
            }
            Err(err) => self.status = format!("Не удалось сгенерировать: {err}"),
        }
    }

    /// Быстрая вставка: разбирает скопированный логин и пароль в новую запись.
    fn quick_add_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
        if !self.quick_open {
            return;
        }

        let mut open = true;
        let mut paste = false;
        let mut parse = false;
        let mut create = false;
        let mut cancel = false;

        egui::Window::new("Быстрая вставка логина и пароля")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    "Вставьте скопированное: «логин пароль», login:password, «сайт логин пароль», адрес вида https://ivan:secret@site.ru или строки «Логин: …» и «Пароль: …».",
                );
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.quick_text)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("например: mail.ru ivan@example.com Pa$$w0rd"),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Взять из буфера обмена").clicked() {
                        paste = true;
                    }
                    if ui.button("Разобрать").clicked() {
                        parse = true;
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                egui::Grid::new("quick_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Название");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_title)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label("Логин");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_username)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label("Пароль");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_password)
                                .desired_width(f32::INFINITY)
                                .password(true),
                        );
                        ui.end_row();

                        ui.label("Сайт");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.quick_url)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                    });

                ui.add_space(8.0);
                let can_create =
                    !self.quick_username.trim().is_empty() || !self.quick_password.is_empty();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_create, egui::Button::new("Создать запись"))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                    if !can_create {
                        ui.label(
                            egui::RichText::new("Вставьте данные и нажмите «Разобрать»")
                                .weak()
                                .small(),
                        );
                    }
                });
            });

        if paste {
            if let Ok(mut cb) = arboard::Clipboard::new()
                && let Ok(text) = cb.get_text()
                && !text.trim().is_empty()
            {
                self.quick_text = text;
                parse = true;
            } else {
                self.status = "В буфере обмена нет текста.".to_string();
            }
        }

        if parse {
            let parsed = parse_credentials(&self.quick_text);
            self.quick_title = parsed.title;
            self.quick_username = parsed.username;
            self.quick_password = parsed.password;
            self.quick_url = parsed.url;
        }

        if create {
            let title = if self.quick_title.trim().is_empty() {
                "Новая запись"
            } else {
                self.quick_title.trim()
            };
            let mut item = Item::new(ItemKind::Login, title);
            item.username = self.quick_username.trim().to_string();
            item.password = self.quick_password.clone();
            item.url = self.quick_url.trim().to_string();
            item.notes = auto_description(&item);
            match vault.add(item.clone()) {
                Ok(id) => {
                    self.selected = Some(id);
                    self.status = format!("Запись «{}» создана.", item.title);
                    self.quick_open = false;
                    self.quick_text.zeroize();
                    self.quick_password.zeroize();
                }
                Err(err) => self.status = format!("Не удалось создать запись: {err}"),
            }
        }

        if cancel {
            open = false;
        }
        if !open {
            self.quick_open = false;
            self.quick_text.zeroize();
            self.quick_password.zeroize();
        }
    }

    /// Палитра быстрого доступа (как Quick Access у 1Password): открывается
    /// глобальной клавишей из любого окна, фильтрует записи, сортирует
    /// подходящие целевому окну наверх и по Enter возвращает фокус окну
    /// и печатает туда логин и пароль.
    fn quick_access_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
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
    }

    /// Массовое добавление осмысленных описаний записям без заметок.
    fn descriptions_dialog(&mut self, ctx: &egui::Context, vault: &mut Vault) {
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

/// Путь экспорта по умолчанию — рядом с хранилищем.
fn default_export_path(vault_path: &Path, format: ExportFormat) -> String {
    vault_path
        .with_file_name(format!("hranitel-export.{}", format.extension()))
        .display()
        .to_string()
}

/// Слово-вердикт по энтропии пароля.
fn strength_word(bits: f64) -> &'static str {
    if bits < 40.0 {
        "слабый"
    } else if bits < 65.0 {
        "средний"
    } else if bits < 90.0 {
        "сильный"
    } else {
        "очень сильный"
    }
}

/// Цвет вердикта: от тревожного красного до спокойного зелёного.
fn strength_color(bits: f64) -> egui::Color32 {
    if bits < 40.0 {
        egui::Color32::from_rgb(200, 70, 70)
    } else if bits < 65.0 {
        egui::Color32::from_rgb(190, 130, 40)
    } else if bits < 90.0 {
        egui::Color32::from_rgb(70, 140, 70)
    } else {
        egui::Color32::from_rgb(40, 110, 60)
    }
}

/// Первая непустая строка заметки — как краткое описание в списке записей.
fn notes_snippet(notes: &str) -> String {
    let line = notes
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut snippet: String = line.chars().take(56).collect();
    if snippet.chars().count() < line.chars().count() {
        snippet.push('…');
    }
    snippet
}

/// Рисует группу находок аудита; клик выбирает запись и закрывает панель.
fn draw_findings(
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

impl eframe::App for App {
    // Фон окна берём из темы, иначе eframe заливает его чёрным.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.tick(ui.ctx());
        self.track_activity(ui.ctx());

        // F1 открывает и закрывает справку в любом месте программы. Событие
        // разбираем напрямую: так работает и при необычных модификаторах.
        // Ctrl+H — запасной вариант для ноутбуков, где F1 занята мультимедиа.
        let f1_pressed = !self.hotkey_capture
            && !self.quick_capture
            && (ui.ctx().input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::F1,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                })
            }) || ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::H)));
        if f1_pressed {
            self.help_open = !self.help_open;
        }

        // Ctrl+F — курсор в поиск; Esc закрывает диалоги и очищает поиск.
        autotype::set_state(self.autotype_state());
        if let Some(message) = autotype::take_message() {
            if message.starts_with("Автоввод") {
                // Нечего печатать — показываем окно, чтобы сообщение точно увидели.
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.status = message.clone();
            self.autotype_notice = Some((message, Instant::now()));
        }

        // Файл, перетащенный в окно, сразу открывает диалог импорта.
        let dropped = ui.ctx().input(|input| {
            input
                .raw
                .dropped_files
                .first()
                .map(|file| file.path().to_path_buf())
        });
        if let Some(path) = dropped {
            self.import_path = path.display().to_string();
            self.import_open = true;
            self.import_result = None;
            self.status = format!("Файл для импорта: {}", path.display());
        }
        if self.vault.is_some()
            && !self.hotkey_capture
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F))
        {
            self.focus_search = true;
        }
        if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
            && !self.help_open
            && !self.hotkey_capture
            && !self.quick_capture
        {
            if self.quick_access.is_some() {
                self.quick_access = None;
            } else if self.quick_open {
                self.quick_open = false;
                self.quick_text.zeroize();
                self.quick_password.zeroize();
            } else if self.gen_open {
                self.gen_open = false;
                self.gen_result.zeroize();
            } else if self.desc_open {
                self.desc_open = false;
            } else if self.password_open {
                self.password_open = false;
            } else if self.export_open {
                self.export_open = false;
            } else if self.audit_open {
                self.audit_open = false;
            } else if !self.query.is_empty() {
                self.query.clear();
            }
        }

        let status_height = 34.0;
        let content_height = (ui.available_height() - status_height).max(0.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        // Занимаем всю высоту, чтобы строка состояния была внизу окна.
                        ui.set_min_height(ui.available_height());
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

        help::show(ui.ctx(), &mut self.help_open);
        self.settings_dialog(ui.ctx());
        self.autotype_overlay(ui.ctx());
    }
}

fn draw_draft(
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

fn app_dir() -> PathBuf {
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

    PathBuf::from(base).join("Hranitel")
}

fn default_vault_path() -> String {
    app_dir().join("vault.opvault").display().to_string()
}

/// Образец фразы устарел, если он составлен для другого языка.
/// После `regenerate()` отметка совпадает с языком, и образец не пересоздаётся.
/// Сравнивать сам текст нельзя: проверка «ascii или нет» всегда была истинной,
/// и фраза пересоздавалась на каждом кадре — выглядело как бесконечный перебор.
fn phrase_sample_stale(mode: GenMode, sample_lang: Option<Wordlist>, language: Wordlist) -> bool {
    mode == GenMode::Phrase && sample_lang != Some(language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_phrase_sample_is_not_regenerated() {
        // После генерации отметка равна языку — образец стабилен.
        assert!(!phrase_sample_stale(
            GenMode::Phrase,
            Some(Wordlist::Russian),
            Wordlist::Russian
        ));
        assert!(!phrase_sample_stale(
            GenMode::Phrase,
            Some(Wordlist::English),
            Wordlist::English
        ));
    }

    #[test]
    fn changing_language_marks_sample_as_stale() {
        assert!(phrase_sample_stale(
            GenMode::Phrase,
            Some(Wordlist::English),
            Wordlist::Russian
        ));
        assert!(phrase_sample_stale(
            GenMode::Phrase,
            None,
            Wordlist::Russian
        ));
        // В режиме случайных символов отметка языка не важна.
        assert!(!phrase_sample_stale(
            GenMode::Chars,
            None,
            Wordlist::Russian
        ));
    }
}

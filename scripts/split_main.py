#!/usr/bin/env python3
"""Разрезание apps/vault-win/src/main.rs на модули (Т-1 из ТЗ 0.13.0).

Функции переносятся дословно, по диапазонам строк: поведение не меняется.
Видимость добавляется pub(crate) для функций, методов и структур.
"""
import re
from pathlib import Path

SRC = Path("/tmp/main.rs.bak")
lines = SRC.read_text(encoding="utf-8").splitlines(keepends=True)
assert len(lines) == 4492, f"ожидалось 4492 строки, есть {len(lines)}"


def seg(a: int, b: int) -> str:
    """Строки с a по b; a поднимается вверх по док-комментариям."""
    while a > 2 and lines[a - 2].strip().startswith("//"):
        a -= 1
    return "".join(lines[a - 1 : b])


def vis(text: str, structs: list[str]) -> str:
    """pub(crate) у всех fn и у перечисленных структур (тип + поля)."""
    text = re.sub(r"^(\s*)fn ", r"\1pub(crate) fn ", text, flags=re.M)
    for name in structs:
        text = text.replace(f"struct {name} {{", f"pub(crate) struct {name} {{")
        m = re.search(rf"pub\(crate\) struct {name} \{{(.*?)^\}}", text, flags=re.S | re.M)
        if m:
            block = m.group(1)
            new_block = re.sub(r"^(\s*)([A-Za-z_][A-Za-z0-9_]*):", r"\1pub(crate) \2:", block, flags=re.M)
            text = text[: m.start(1)] + new_block + text[m.end(1) :]
    return text


USE_BLOCK = """use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use uuid::Uuid;
use vault_core::{
    ExportFormat, Field, GeneratorOptions, ImportFormat, Item, ItemKind, MatchStrength,
    PassphraseOptions, Separator, Totp, Vault, Wordlist, auto_description, expiries,
    generate_passphrase, generate_password, generate_wrapping_key, import_file, normalize_host,
    now_secs, parse_credentials, passphrase_entropy_bits, password_entropy_bits,
    protected_wrapping_key, registrable_domain, sidecar_path,
};
use zeroize::{Zeroize, Zeroizing};

use crate::*;
"""

out = Path("/home/z/my-project/apps/vault-win/src")

# ---------------------------------------------------------------- theme.rs
theme = "//! Оформление окна: светлые и тёмные визуалы, шрифты.\n\n" + USE_BLOCK + "\n" + seg(119, 250) + "\n" + seg(4233, 4257)
theme = vis(theme, [])
theme = theme.replace("enum Theme {", "pub(crate) enum Theme {")
(out / "theme.rs").write_text(theme, encoding="utf-8")

# ---------------------------------------------------------------- settings.rs
settings = "//! Настройки программы: settings.txt, каталог приложения.\n\n" + USE_BLOCK + "\n" + seg(252, 360) + "\n" + seg(4259, 4277)
settings = vis(settings, ["Settings"])
(out / "settings.rs").write_text(settings, encoding="utf-8")

# ---------------------------------------------------------------- winutil.rs
winutil = """//! Утилиты Windows: окно, автозапуск, один экземпляр, проверка адресов.

use windows::core::w;

use crate::*;

""" + seg(82, 111) + "\n" + seg(4287, 4434)
winutil = vis(winutil, [])
winutil = winutil.replace("static INSTANCE", "pub(crate) static INSTANCE")
(out / "winutil.rs").write_text(winutil, encoding="utf-8")

# ---------------------------------------------------------------- state.rs
state = "//! Состояние приложения: структура App, блокировка, жизненный цикл.\n\n" + USE_BLOCK + "\n" + seg(113, 117) + "\n" + seg(126, 131) + "\n" + seg(362, 569) + "\n" + seg(571, 1236) + "\n" + seg(2993, 2999) + "}\n\n" + seg(4278, 4284)
state = vis(state, ["App", "Draft", "QuickAccess"])
for name in ("Mode", "GenMode", "KindGroup", "DraftOutcome", "BrowserAsk", "BrowserOutcome"):
    state = state.replace(f"enum {name} {{", f"pub(crate) enum {name} {{")
(out / "state.rs").write_text(state, encoding="utf-8")

# ---------------------------------------------------------------- ui/
(out / "ui").mkdir(exist_ok=True)
ui_mod = """//! Отрисовка главного окна: экраны, список, карточка, статус-бар.

mod browser;
mod main_view;
mod quick_access;
mod statusbar;
mod unlock;

use crate::*;

""" + seg(3762, 3767) + "\n" + seg(3770, 3807)
ui_mod = vis(ui_mod, [])
(out / "ui" / "mod.rs").write_text(ui_mod, encoding="utf-8")


def impl_file(doc: str, body: str) -> str:
    body = vis(body, [])
    return f"{doc}\n\nuse crate::*;\n\nimpl App {{\n{body}}}\n"


(out / "ui" / "unlock.rs").write_text(
    impl_file("//! Экран входа: файл хранилища, мастер-пароль, Windows Hello.", seg(1237, 1360)),
    encoding="utf-8",
)
(out / "ui" / "main_view.rs").write_text(
    impl_file("//! Основной экран: тулбар, категории, список записей, карточка.", seg(1362, 2047)),
    encoding="utf-8",
)
statusbar_body = "    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {\n" + seg(3976, 4013) + "    }\n"
(out / "ui" / "statusbar.rs").write_text(
    impl_file("//! Строка состояния внизу окна.", statusbar_body),
    encoding="utf-8",
)
(out / "ui" / "browser.rs").write_text(
    impl_file("//! Мост к браузерному расширению: запросы, диалоги, сохранение.", seg(3002, 3450)),
    encoding="utf-8",
)
(out / "ui" / "quick_access.rs").write_text(
    impl_file("//! Палитра быстрого доступа.", seg(3451, 3666)),
    encoding="utf-8",
)

# ---------------------------------------------------------------- dialogs/
(out / "dialogs").mkdir(exist_ok=True)
(out / "dialogs" / "mod.rs").write_text(
    """//! Диалоги приложения.

mod audit;
mod describe;
mod draft;
mod export;
mod generator;
mod password;
mod quick_add;
mod settings_dlg;
""",
    encoding="utf-8",
)
(out / "dialogs" / "settings_dlg.rs").write_text(
    impl_file("//! Диалог настроек.", seg(2049, 2366)), encoding="utf-8"
)
(out / "dialogs" / "password.rs").write_text(
    impl_file("//! Диалог смены мастер-пароля.", seg(2368, 2469)), encoding="utf-8"
)
(out / "dialogs" / "export.rs").write_text(
    impl_file("//! Диалог экспорта.", seg(2471, 2595)), encoding="utf-8"
)
(out / "dialogs" / "audit.rs").write_text(
    impl_file("//! Панель аудита хранилища.", seg(2597, 2667) + "\n" + seg(3810, 3838)),
    encoding="utf-8",
)
(out / "dialogs" / "generator.rs").write_text(
    impl_file("//! Окно генератора паролей.", seg(2669, 2840)), encoding="utf-8"
)
(out / "dialogs" / "quick_add.rs").write_text(
    impl_file("//! Быстрая вставка: разбор скопированного логина и пароля.", seg(2842, 2991)),
    encoding="utf-8",
)
(out / "dialogs" / "describe.rs").write_text(
    impl_file("//! Массовое добавление осмысленных описаний.", seg(3668, 3758)), encoding="utf-8"
)
draft_rs = "//! Диалог создания и правки записи.\n\n" + USE_BLOCK + "\n" + vis(seg(4021, 4231), [])
(out / "dialogs" / "draft.rs").write_text(draft_rs, encoding="utf-8")

# ---------------------------------------------------------------- main.rs
main_rs = """//! Оконный клиент «Хранитель» на eframe/egui.
//!
//! Вся криптография и работа с файлом — в `vault-core`; здесь только UI,
//! буфер обмена с автоочисткой и автоблокировка. Устройство оконного
//! приложения: `state.rs` (состояние), `theme.rs` (оформление),
//! `settings.rs` (настройки), `ui/` (экраны) и `dialogs/` (диалоги).

// В релизе не открываем консольное окно рядом с интерфейсом: оно забирает
// фокус и клавиши (включая F1). В отладочной сборке консоль остаётся для логов.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autotype;
mod browser;
mod dialogs;
mod filedialog;
mod hello_win;
mod help;
mod settings;
mod state;
mod theme;
mod tray;
mod uia;
mod ui;
mod winutil;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use uuid::Uuid;
use vault_core::{
    ExportFormat, Field, GeneratorOptions, ImportFormat, Item, ItemKind, MatchStrength,
    PassphraseOptions, Separator, Totp, Vault, Wordlist, auto_description, expiries,
    generate_passphrase, generate_password, generate_wrapping_key, import_file, normalize_host,
    now_secs, parse_credentials, passphrase_entropy_bits, password_entropy_bits,
    protected_wrapping_key, registrable_domain, sidecar_path,
};
use zeroize::{Zeroize, Zeroizing};

pub(crate) use settings::*;
pub(crate) use state::*;
pub(crate) use theme::*;
pub(crate) use ui::*;
pub(crate) use winutil::*;

pub(crate) const APP_TITLE: &str = "Хранитель";
pub(crate) const AUTO_LOCK: Duration = Duration::from_secs(5 * 60);
pub(crate) const CLIPBOARD_TTL: Duration = Duration::from_secs(30);

""" + seg(39, 80) + "\n" + seg(3840, 3975) + "        self.status_bar(ui);\n\n" + seg(4014, 4019) + "\n" + seg(4436, 4492)
(out / "main.rs").write_text(main_rs, encoding="utf-8")

print("разрезано: main.rs, state.rs, settings.rs, theme.rs, winutil.rs, ui/ (6), dialogs/ (9)")

//! Отрисовка главного окна: экраны, список, карточка, статус-бар.

mod browser;
mod main_view;
mod quick_access;
mod statusbar;
mod unlock;

use crate::*;

/// Путь экспорта по умолчанию — рядом с хранилищем.
pub(crate) fn default_export_path(vault_path: &Path, format: ExportFormat) -> String {
    vault_path
        .with_file_name(format!("hranitel-export.{}", format.extension()))
        .display()
        .to_string()
}

/// Слово-вердикт по энтропии пароля.
pub(crate) fn strength_word(bits: f64) -> &'static str {
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
pub(crate) fn strength_color(bits: f64) -> egui::Color32 {
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
pub(crate) fn notes_snippet(notes: &str) -> String {
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

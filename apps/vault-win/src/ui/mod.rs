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

/// Акцентный цвет интерфейса (как выделение в теме).
pub(crate) fn accent_color(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().selection.bg_fill
}

/// Янтарный цвет предупреждений (сроки, аудит).
pub(crate) const AMBER: egui::Color32 = egui::Color32::from_rgb(192, 122, 30);
/// Зелёный цвет «всё хорошо» (TOTP, аудит без находок).
pub(crate) const GREEN: egui::Color32 = egui::Color32::from_rgb(46, 139, 87);
/// Красный цвет просроченного.
pub(crate) const RED: egui::Color32 = egui::Color32::from_rgb(192, 57, 43);

/// Первая буква типа записи — для аватара строки списка (И-3).
pub(crate) fn kind_letter(kind: ItemKind) -> char {
    kind.label().chars().next().unwrap_or('•')
}

/// Шкала стойкости пароля из пяти сегментов (И-4): сколько сегментов
/// залито и каким цветом, определяет энтропия.
pub(crate) fn strength_scale(ui: &mut egui::Ui, bits: f64) {
    let filled = if bits < 40.0 {
        1
    } else if bits < 65.0 {
        2
    } else if bits < 90.0 {
        3
    } else if bits < 120.0 {
        4
    } else {
        5
    };
    ui.horizontal(|ui| {
        for idx in 0..5 {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 6.0), egui::Sense::hover());
            let radius = egui::CornerRadius::same(3);
            let color = if idx < filled {
                strength_color(bits)
            } else {
                ui.visuals().faint_bg_color
            };
            ui.painter().rect_filled(rect, radius, color);
        }
    });
}

/// Кольцо обратного отсчёта одноразового кода (И-4): дуга уменьшается,
/// пока живёт код, в центре — оставшиеся секунды.
pub(crate) fn totp_ring(ui: &mut egui::Ui, seconds_left: u32, period: u32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
    let center = rect.center();
    let radius = 14.0;
    ui.painter().circle_stroke(
        center,
        radius,
        egui::Stroke::new(3.0, ui.visuals().faint_bg_color),
    );
    let fraction = if period == 0 {
        0.0
    } else {
        (seconds_left as f32 / period as f32).clamp(0.0, 1.0)
    };
    if fraction > 0.004 {
        let start = -std::f32::consts::FRAC_PI_2;
        let sweep = fraction * std::f32::consts::TAU;
        let steps = 48;
        let points: Vec<egui::Pos2> = (0..=steps)
            .map(|i| {
                let angle = start + sweep * (i as f32 / steps as f32);
                center + egui::vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        ui.painter().add(egui::epaint::PathShape::line(
            points,
            egui::Stroke::new(3.0, accent_color(ui)),
        ));
    }
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        seconds_left.to_string(),
        egui::FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
}

/// Дата из unix-секунд в виде ГГГГ-ММ-ДД (для имён резервных копий, Ф-5).
pub(crate) fn date_ymd(unix_secs: i64) -> String {
    // Дни от эпохи → григорианская дата (алгоритм Гаусса, без внешних
    // зависимостей; годы 1901–2099 покрывают любые реальные файлы).
    let days = unix_secs.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Перевод дней от эпохи в григорианскую дату (Howard Hinnant, days_from_civil в обратную сторону).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ymd_of_known_dates() {
        assert_eq!(date_ymd(0), "1970-01-01");
        assert_eq!(date_ymd(1_761_958_400), "2025-11-01"); // 01.11.2025
        assert_eq!(date_ymd(951_782_400), "2000-02-29"); // високосный год
    }
}

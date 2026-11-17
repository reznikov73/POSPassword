//! Оформление окна: светлые и тёмные визуалы, шрифты.

use std::sync::Arc;

use eframe::egui;

/// Оформление окна. Выбор сохраняется между запусками.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Theme::Light => "светлая",
            Theme::Dark => "тёмная",
        }
    }

    pub(crate) fn toggled(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    pub(crate) fn apply(self, ctx: &egui::Context) {
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

pub(crate) fn theme_visuals(light: bool) -> egui::Visuals {
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

pub(crate) fn install_fonts(ctx: &egui::Context) {
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

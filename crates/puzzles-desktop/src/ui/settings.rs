//! The Settings dialog (themes decision in `docs/porting-plan.md`):
//! Appearance, and the open game's preferences. Changes apply on OK.

use eframe::egui;
use egui::{Id, Key, RichText};
use puzzles_core::config::ConfigItem;
use puzzles_theme::{Rgb, UiColours, mix};

use super::dialogs::{fields, keep_popups_above, ok_cancel};
use super::puzzle_view::colour;
use crate::app::settings::Settings;
use crate::app::themes::Themes;

/// The open Settings dialog: edited copies of what it shows.
#[derive(Debug)]
pub struct SettingsDialog {
    settings: Settings,
    /// The open game's name and preferences, if it has any.
    game: Option<(&'static str, Vec<ConfigItem>)>,
    /// Where user themes go, to tell the user.
    themes_dir: Option<String>,
    error: Option<String>,
}

/// What the user did with the dialog this frame.
#[derive(Debug, PartialEq)]
pub enum SettingsAction {
    /// Still open.
    Open,
    /// OK: use these settings and, if a game is open, these preferences.
    Apply(Settings, Option<Vec<ConfigItem>>),
    /// Cancelled.
    Close,
}

impl SettingsDialog {
    /// A dialog showing `settings` and the game's preferences.
    #[must_use]
    pub fn new(
        settings: Settings,
        game: Option<(&'static str, Vec<ConfigItem>)>,
        themes_dir: Option<String>,
    ) -> Self {
        Self {
            settings,
            game,
            themes_dir,
            error: None,
        }
    }

    /// Shows why the last OK was refused.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Draws the dialog.
    pub fn show(&mut self, ctx: &egui::Context, themes: &Themes) -> SettingsAction {
        let mut action = SettingsAction::Open;
        let enter = ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.any());
        let response = egui::Modal::new(Id::new("settings-dialog")).show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.heading("Settings");
            ui.add_space(8.0);
            ui.label(RichText::new("Appearance").strong());
            ui.checkbox(&mut self.settings.follow_system, "Follow system appearance");
            egui::Grid::new("appearance")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let s = &mut self.settings;
                    if s.follow_system {
                        theme_choice(
                            ui,
                            "Light theme",
                            &mut s.light_theme,
                            &themes.names(Some(false)),
                        );
                        theme_choice(
                            ui,
                            "Dark theme",
                            &mut s.dark_theme,
                            &themes.names(Some(true)),
                        );
                    } else {
                        theme_choice(ui, "Theme", &mut s.theme, &themes.names(None));
                    }
                });
            if let Some(dir) = &self.themes_dir {
                ui.label(
                    RichText::new(format!("Your own themes: {dir}"))
                        .small()
                        .weak(),
                );
            }
            for error in themes.errors() {
                ui.label(
                    RichText::new(error)
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
            if let Some((name, items)) = &mut self.game {
                ui.add_space(12.0);
                ui.label(RichText::new(format!("{name} preferences")).strong());
                fields(ui, "game-prefs", items, false, false);
            }
            if let Some(error) = &self.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(ui.visuals().error_fg_color));
            }
            ui.add_space(10.0);
            let (ok, cancel) = ok_cancel(ui);
            if cancel {
                action = SettingsAction::Close;
            } else if ok || enter {
                let prefs = self.game.as_ref().map(|(_, items)| items.clone());
                action = SettingsAction::Apply(self.settings.clone(), prefs);
            }
        });
        keep_popups_above(ctx, Id::new("settings-dialog"));
        if action == SettingsAction::Open && response.should_close() {
            action = SettingsAction::Close;
        }
        action
    }
}

/// A labelled drop-down of theme names.
fn theme_choice(ui: &mut egui::Ui, label: &str, value: &mut String, names: &[String]) {
    let label_id = ui.label(label).id;
    egui::ComboBox::from_id_salt(label)
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for name in names {
                ui.selectable_value(value, name.clone(), name.as_str());
            }
        })
        .response
        .labelled_by(label_id);
    ui.end_row();
}

/// egui's colours for a theme: its light or dark style, with panels,
/// text and selections from the theme.
#[must_use]
pub fn visuals(c: UiColours) -> egui::Visuals {
    let mut v = if c.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let panel = colour(c.panel);
    v.panel_fill = panel;
    v.window_fill = panel;
    v.override_text_color = Some(colour(c.text));
    let accent = colour(c.accent);
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent.gamma_multiply(0.45);
    v.selection.stroke.color = accent;
    // Controls stand out from the panel by moving towards the text
    // colour: egui's own greys can equal the panel (Classic's 0.9 grey
    // hid tick boxes and drop-downs).
    let toward_text = |t: f32| colour(mix(c.panel, c.text, t));
    for (state, t) in [
        (&mut v.widgets.inactive, 0.10),
        (&mut v.widgets.hovered, 0.16),
        (&mut v.widgets.active, 0.22),
        (&mut v.widgets.open, 0.16),
    ] {
        state.bg_fill = toward_text(t);
        state.weak_bg_fill = toward_text(t);
    }
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, toward_text(0.35));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, toward_text(0.2));
    let away = if c.dark { Rgb::BLACK } else { Rgb::WHITE };
    v.extreme_bg_color = colour(mix(c.panel, away, 0.5));
    v.faint_bg_color = toward_text(0.04);
    v
}

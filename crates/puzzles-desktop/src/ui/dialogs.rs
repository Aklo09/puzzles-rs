//! The configuration dialog: a modal form generated from a game's
//! `ConfigItem`s, for Custom parameters, game IDs, random seeds and
//! preferences (dialogs and presets decision in `docs/porting-plan.md`).
//! The game's error message shows in the dialog, which stays open.

use eframe::egui;
use egui::{Id, Key, RichText};
use puzzles_core::config::{ConfigItem, ConfigValue};
use puzzles_core::midend::{Config, ConfigKind};

/// An open dialog.
#[derive(Debug)]
pub struct Dialog {
    kind: ConfigKind,
    title: String,
    items: Vec<ConfigItem>,
    error: Option<String>,
    /// Focus the first text field on the first frame.
    fresh: bool,
}

/// What the user did with the dialog this frame.
#[derive(Debug, PartialEq)]
pub enum DialogAction {
    /// Still open.
    Open,
    /// OK: apply these fields.
    Apply(ConfigKind, Vec<ConfigItem>),
    /// Cancelled.
    Close,
}

impl Dialog {
    /// A dialog for `config`, of kind `kind`.
    #[must_use]
    pub fn new(kind: ConfigKind, config: Config) -> Self {
        Self {
            kind,
            title: config.title,
            items: config.items,
            error: None,
            fresh: true,
        }
    }

    /// Shows why the last OK was refused.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Draws the dialog.
    pub fn show(&mut self, ctx: &egui::Context) -> DialogAction {
        let mut action = DialogAction::Open;
        // Read Enter before the fields: a text field would take it.
        let enter = ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.any());
        let response = egui::Modal::new(Id::new("config-dialog")).show(ctx, |ui| {
            ui.set_min_width(320.0);
            ui.heading(&self.title);
            ui.add_space(8.0);
            let wide = matches!(self.kind, ConfigKind::Seed | ConfigKind::Desc);
            egui::Grid::new("config-items")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let mut first_text = self.fresh;
                    for item in &mut self.items {
                        // Each field is named by its label, for screen
                        // readers (and tests).
                        let label = ui.label(item.name).id;
                        match &mut item.value {
                            ConfigValue::String(text) => {
                                let width = if wide { 360.0 } else { 120.0 };
                                let mut edit = egui::TextEdit::singleline(text)
                                    .desired_width(width)
                                    .id_salt(item.name)
                                    .show(ui);
                                let response = edit.response.response.clone().labelled_by(label);
                                if first_text {
                                    // Focused with its text selected, so
                                    // typing replaces it.
                                    response.request_focus();
                                    let all = egui::text::CCursorRange::select_all(&edit.galley);
                                    edit.state.cursor.set_char_range(Some(all));
                                    edit.state.store(ui.ctx(), response.id);
                                    first_text = false;
                                }
                            }
                            ConfigValue::Choices {
                                names, selected, ..
                            } => {
                                egui::ComboBox::from_id_salt(item.name)
                                    .selected_text(names.get(*selected).copied().unwrap_or(""))
                                    .show_ui(ui, |ui| {
                                        for (i, name) in names.iter().enumerate() {
                                            ui.selectable_value(selected, i, *name);
                                        }
                                    })
                                    .response
                                    .labelled_by(label);
                            }
                            ConfigValue::Bool(on) => {
                                ui.checkbox(on, "").labelled_by(label);
                            }
                        }
                        ui.end_row();
                    }
                });
            self.fresh = false;
            if let Some(error) = &self.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(ui.visuals().error_fg_color));
            }
            ui.add_space(10.0);
            // Bottom right, in the OS's order: OK last on macOS, first
            // elsewhere. `Sides` keeps the row one line high.
            egui::Sides::new().show(
                ui,
                |_| {},
                |ui| {
                    // The right side is laid out right to left.
                    let (ok, cancel) = if cfg!(target_os = "macos") {
                        let ok = ui.button("OK").clicked();
                        (ok, ui.button("Cancel").clicked())
                    } else {
                        let cancel = ui.button("Cancel").clicked();
                        (ui.button("OK").clicked(), cancel)
                    };
                    if cancel {
                        action = DialogAction::Close;
                    } else if ok || enter {
                        action = DialogAction::Apply(self.kind, self.items.clone());
                    }
                },
            );
        });
        if action == DialogAction::Open && response.should_close() {
            action = DialogAction::Close;
        }
        action
    }
}

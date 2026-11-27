//! The puzzle area: sizes the session to it, shows the canvas as a
//! texture (uploading only what changed), and turns input into game
//! events.

use eframe::egui;
use egui::{
    Color32, ColorImage, Event, Id, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2,
};
use puzzles_canvas::Canvas;
use puzzles_canvas::tiny_skia::IntRect;
use puzzles_core::button::{Button, MouseAction, MouseButton};
use puzzles_theme::Rgb;

use super::input;
use crate::app::layout;
use crate::app::session::{Outcome, Session};

/// The texture holding the canvas, and the canvas version it shows.
#[derive(Default)]
pub struct PuzzleView {
    texture: Option<(TextureHandle, u64)>,
    /// The mouse button held, so moves become drags of it.
    held: Option<MouseButton>,
    /// Where the puzzle was drawn last, in points.
    shown: Option<Rect>,
}

/// The canvas pixels in `rect`, as an egui image.
fn image(canvas: &Canvas, rect: IntRect) -> ColorImage {
    let stride = canvas.width() as usize * 4;
    let (x, w) = (rect.x() as usize * 4, rect.width() as usize * 4);
    let data = canvas.pixmap().data();
    let mut rgba = Vec::with_capacity(w * rect.height() as usize);
    for y in rect.y() as usize..rect.bottom() as usize {
        rgba.extend_from_slice(&data[y * stride + x..y * stride + x + w]);
    }
    // The canvas is opaque, so premultiplied and straight are the same.
    ColorImage::from_rgba_premultiplied([rect.width() as usize, rect.height() as usize], &rgba)
}

/// An egui colour for a theme colour.
pub fn colour(rgb: Rgb) -> Color32 {
    let [r, g, b] = rgb.to_rgb8();
    Color32::from_rgb(r, g, b)
}

impl PuzzleView {
    /// Where the puzzle was drawn last, in points.
    #[cfg(test)]
    pub fn shown(&self) -> Option<Rect> {
        self.shown
    }

    /// Copies what changed on the canvas to the texture.
    fn upload(&mut self, ctx: &egui::Context, canvas: &mut Canvas, version: u64) {
        let options = TextureOptions::NEAREST;
        match &mut self.texture {
            Some((texture, shown)) if *shown == version => {
                if let Some(dirty) = canvas.take_dirty() {
                    let pos = [dirty.x() as usize, dirty.y() as usize];
                    texture.set_partial(pos, image(canvas, dirty), options);
                }
            }
            _ => {
                canvas.take_dirty();
                let Some(all) = IntRect::from_xywh(0, 0, canvas.width(), canvas.height()) else {
                    return;
                };
                let full = image(canvas, all);
                match &mut self.texture {
                    Some((texture, shown)) => {
                        texture.set(full, options);
                        *shown = version;
                    }
                    None => {
                        self.texture = Some((ctx.load_texture("puzzle", full, options), version));
                    }
                }
            }
        }
    }

    /// Draws the puzzle in the remaining space of `ui` and feeds it the
    /// frame's input.
    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session, mac: bool) -> Outcome {
        let rect = ui.available_rect_before_wrap();
        let id = Id::new("puzzle");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        // Keep the keyboard, arrows and Tab included, unless a dialog
        // wants it.
        if !ui.ctx().egui_wants_keyboard_input() {
            ui.memory_mut(|m| {
                m.request_focus(id);
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }

        let ppp = ui.ctx().pixels_per_point();
        if let Some(area) = layout::available_pixels((rect.width(), rect.height()), ppp) {
            session.layout(area, ppp);
        }
        session.redraw();

        let mut origin = None;
        if let Some((canvas, version)) = session.canvas_mut() {
            self.upload(ui.ctx(), canvas, version);
            let size = (canvas.width(), canvas.height());
            let area =
                layout::available_pixels((rect.width(), rect.height()), ppp).unwrap_or((0, 0));
            let (ox, oy) = layout::centred(area, size);
            // On whole physical pixels, so the texture maps 1:1.
            let min = Pos2::new(
                ((rect.min.x * ppp).round() + ox as f32) / ppp,
                ((rect.min.y * ppp).round() + oy as f32) / ppp,
            );
            let shown = Rect::from_min_size(min, Vec2::new(size.0 as f32, size.1 as f32) / ppp);
            if let Some((texture, _)) = &self.texture {
                ui.painter().image(
                    texture.id(),
                    shown,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            origin = Some(min);
            self.shown = Some(shown);
        }

        let Some(origin) = origin else {
            return Outcome::Continue;
        };
        self.feed_input(ui, session, &response, origin, ppp, mac)
    }

    fn feed_input(
        &mut self,
        ui: &egui::Ui,
        session: &mut Session,
        response: &egui::Response,
        origin: Pos2,
        ppp: f32,
        mac: bool,
    ) -> Outcome {
        let keyboard = response.has_focus();
        let events = ui.input(|i| i.events.clone());
        let at = |pos: Pos2| layout::to_canvas((pos.x - origin.x, pos.y - origin.y), ppp);
        let mut outcome = Outcome::Continue;
        let mut send = |session: &mut Session, (x, y): (i32, i32), button: Button| {
            if session.input(x, y, button) == Outcome::Quit {
                outcome = Outcome::Quit;
            }
        };
        for event in events {
            match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if keyboard => {
                    if let Some(button) = input::key_button(key, modifiers) {
                        send(session, (0, 0), button);
                    }
                }
                Event::Text(text) if keyboard => {
                    for button in input::text_buttons(&text) {
                        send(session, (0, 0), button);
                    }
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let Some(mouse) = input::mouse_button(button, modifiers, mac) else {
                        continue;
                    };
                    if pressed {
                        if response.contains_pointer() {
                            self.held = Some(mouse);
                            send(session, at(pos), Button::mouse(mouse, MouseAction::Press));
                        }
                    } else if self.held.is_some() {
                        // The mid-end releases the button that was
                        // pressed, whichever this is.
                        self.held = None;
                        send(session, at(pos), Button::mouse(mouse, MouseAction::Release));
                    }
                }
                Event::PointerMoved(pos) => {
                    if let Some(held) = self.held {
                        send(session, at(pos), Button::mouse(held, MouseAction::Drag));
                    }
                }
                _ => {}
            }
        }
        outcome
    }
}

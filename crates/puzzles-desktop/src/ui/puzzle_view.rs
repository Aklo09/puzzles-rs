//! The puzzle area: sizes the session to it (zoomed, then scrolled),
//! shows the canvas as a texture (uploading only what changed), and
//! turns input into game events.

use eframe::egui;
use egui::scroll_area::{DragScroll, ScrollSource};
use egui::{
    Color32, ColorImage, Event, Id, Key, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2,
};
use puzzles_canvas::Canvas;
use puzzles_canvas::tiny_skia::IntRect;
use puzzles_core::button::{Button, MouseAction, MouseButton};
use puzzles_theme::Rgb;

use super::input;
use crate::app::layout;
use crate::app::session::{Outcome, Session};
use crate::app::zoom::{self, Zoom};

/// The point a zoom keeps in place.
#[derive(Clone, Copy)]
enum Anchor {
    /// The middle of the view (keyboard and menu).
    Centre,
    /// The pointer, at this offset from the view's corner (points).
    At(Vec2),
}

/// Where the puzzle is on screen this frame, for input.
struct Placement {
    /// The part of the puzzle area not covered by scroll bars.
    visible: Rect,
    /// The canvas's top-left corner.
    origin: Pos2,
    pixels_per_point: f32,
}

/// The texture holding the canvas, and the canvas version it shows.
#[derive(Default)]
pub struct PuzzleView {
    texture: Option<(TextureHandle, u64)>,
    /// The mouse button held, so moves become drags of it.
    held: Option<MouseButton>,
    /// Where the puzzle was drawn last, in points.
    shown: Option<Rect>,
    zoom: Zoom,
    /// The zoom applied last (capped), and whether it can grow.
    applied: (f32, bool),
    /// A zoom change waiting for this frame's layout, and the point to
    /// keep in place.
    rezoomed: Option<Anchor>,
    /// The scroll position last frame, in points.
    offset: Vec2,
    /// The game didn't use an Escape key (for immersive mode).
    unused_escape: bool,
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

    /// The zoom applied last (over fit to window), and whether zooming in
    /// can go further.
    pub fn applied_zoom(&self) -> (f32, bool) {
        self.applied
    }

    /// Changes the zoom by a step, or back to fit, keeping the middle of
    /// the view in place.
    pub fn zoom_command(&mut self, f: impl FnOnce(&mut Zoom) -> bool) {
        if f(&mut self.zoom) {
            self.rezoomed = Some(Anchor::Centre);
        }
    }

    /// True once after the game left an Escape unused.
    pub fn take_unused_escape(&mut self) -> bool {
        std::mem::take(&mut self.unused_escape)
    }

    /// Copies what changed on the canvas to the texture.
    fn upload_to(
        texture: &mut Option<(TextureHandle, u64)>,
        ctx: &egui::Context,
        canvas: &mut Canvas,
        version: u64,
    ) {
        let options = TextureOptions::NEAREST;
        match texture {
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
                match texture {
                    Some((texture, shown)) => {
                        texture.set(full, options);
                        *shown = version;
                    }
                    None => {
                        *texture = Some((ctx.load_texture("puzzle", full, options), version));
                    }
                }
            }
        }
    }

    /// Draws the puzzle in the remaining space of `ui` and feeds it the
    /// frame's input.
    /// Without `input` (a dialog is open), the puzzle only draws.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        session: &mut Session,
        mac: bool,
        input: bool,
    ) -> Outcome {
        let rect = ui.available_rect_before_wrap();
        let id = Id::new("puzzle");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        // Keep the keyboard, arrows and Tab included, unless a dialog
        // (no `input`) or a menu needs it. (Not `egui_wants_keyboard_input`:
        // that is true whenever any widget has focus, the puzzle
        // included, which would drop the lock below after one frame.)
        if input && !egui::Popup::is_any_open(ui.ctx()) {
            ui.memory_mut(|m| {
                // Only when it has lost focus: a request replaces egui's
                // focus record, dropping the lock below, and egui then
                // moves focus between widgets on the next arrow key.
                if !m.has_focus(id) {
                    m.request_focus(id);
                }
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
        let Some(view) = layout::available_pixels((rect.width(), rect.height()), ppp) else {
            return Outcome::Continue;
        };
        // Pinch and Cmd/Ctrl+wheel zoom around the pointer.
        if input && let Some(pointer) = response.hover_pos() {
            let by = ui.input(egui::InputState::zoom_delta);
            if (by - 1.0).abs() > f32::EPSILON && self.zoom.scale(by) {
                self.rezoomed = Some(Anchor::At(pointer - rect.min));
            }
        }
        let max_side = zoom::MAX_SIDE.min(ui.input(|i| i.max_texture_side) as u32);
        let canvas_points = |session: &mut Session| {
            session
                .canvas_mut()
                .map(|(c, _)| Vec2::new(c.width() as f32, c.height() as f32) / ppp)
        };
        let old_size = canvas_points(session);
        session.layout(self.zoom.area(view, max_side), ppp);
        session.redraw();
        let applied = self.zoom.applied(view, max_side);
        self.applied = (applied, applied < Zoom::ceiling(view, max_side));
        if applied < self.zoom.factor() {
            // Capped here: zooming out starts from what shows.
            self.zoom.scale(applied / self.zoom.factor());
        }

        let mut scroll = egui::ScrollArea::both()
            .id_salt("puzzle-scroll")
            .auto_shrink(false)
            // Every mouse button is the game's: no dragging to scroll.
            .scroll_source(ScrollSource {
                scroll_bar: true,
                drag: DragScroll::Never,
                mouse_wheel: true,
            });
        if let (Some(anchor), Some(old), Some(new)) =
            (self.rezoomed.take(), old_size, canvas_points(session))
        {
            let anchor = match anchor {
                Anchor::Centre => rect.size() / 2.0,
                Anchor::At(at) => at,
            };
            let axis =
                |view, old, new, offset, at| zoom::anchored_offset(view, old, new, offset, at);
            scroll = scroll.scroll_offset(Vec2::new(
                axis(rect.width(), old.x, new.x, self.offset.x, anchor.x),
                axis(rect.height(), old.y, new.y, self.offset.y, anchor.y),
            ));
        }
        // Solid scroll bars sit beside the puzzle, so clicks on them never
        // reach the game; and no fading at the edges, which would dim it.
        let mut style = egui::style::ScrollStyle::solid();
        style.fade.strength = 0.0;
        ui.style_mut().spacing.scroll = style;

        let mut origin = None;
        let texture = &mut self.texture;
        let shown = &mut self.shown;
        let output = scroll.show_viewport(ui, |ui, viewport| {
            let Some((canvas, version)) = session.canvas_mut() else {
                return;
            };
            Self::upload_to(texture, ui.ctx(), canvas, version);
            let size = (canvas.width(), canvas.height());
            let size_points = Vec2::new(size.0 as f32, size.1 as f32) / ppp;
            ui.set_min_size(size_points);
            let visible = layout::available_pixels((viewport.width(), viewport.height()), ppp)
                .unwrap_or((0, 0));
            let (ox, oy) = layout::centred(visible, size);
            // On whole physical pixels, so the texture maps 1:1.
            let content = ui.max_rect().min;
            let min = Pos2::new(
                ((content.x * ppp).round() + ox as f32) / ppp,
                ((content.y * ppp).round() + oy as f32) / ppp,
            );
            let rect = Rect::from_min_size(min, size_points);
            if let Some((texture, _)) = texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            origin = Some(min);
            *shown = Some(rect);
        });
        self.offset = output.state.offset;
        let inner = output.inner_rect;

        let Some(origin) = origin.filter(|_| input) else {
            self.held = None;
            return Outcome::Continue;
        };
        let placement = Placement {
            visible: inner,
            origin,
            pixels_per_point: ppp,
        };
        self.feed_input(ui, session, &response, &placement, mac)
    }

    fn feed_input(
        &mut self,
        ui: &egui::Ui,
        session: &mut Session,
        response: &egui::Response,
        placement: &Placement,
        mac: bool,
    ) -> Outcome {
        let Placement {
            visible,
            origin,
            pixels_per_point: ppp,
        } = *placement;
        let keyboard = response.has_focus();
        let events = ui.input(|i| i.events.clone());
        let at = |pos: Pos2| layout::to_canvas((pos.x - origin.x, pos.y - origin.y), ppp);
        let mut outcome = Outcome::Continue;
        let mut send = |session: &mut Session, (x, y): (i32, i32), button: Button| {
            let result = session.input(x, y, button);
            if result == Outcome::Quit {
                outcome = Outcome::Quit;
            }
            result
        };
        for event in events {
            match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if keyboard => {
                    if let Some(button) = input::key_button(key, modifiers)
                        && send(session, (0, 0), button) == Outcome::Unused
                        && key == Key::Escape
                    {
                        self.unused_escape = true;
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
                        if response.contains_pointer() && visible.contains(pos) {
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

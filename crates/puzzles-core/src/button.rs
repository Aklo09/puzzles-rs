//! Input events passed to games: C's `int button` codes (`puzzles.h`,
//! upstream 616da16) as a typed [`Button`], plus `button2label` from
//! `misc.c`.
//!
//! C packs a key or mouse event and its modifier flags into one `int`.
//! Here the event is a [`ButtonKind`] and the flags are [`Modifiers`].
//! [`Button::from_raw`] and [`Button::to_raw`] convert exactly to and
//! from C's values, which the oracle tests (and the mid-end's
//! button-canonicalising code, checked against C) rely on.

use bitflags::bitflags;

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// C's `LEFT_*`.
    Left,
    /// C's `MIDDLE_*`.
    Middle,
    /// C's `RIGHT_*`.
    Right,
}

/// What a mouse button did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseAction {
    /// C's `*_BUTTON`: pressed.
    Press,
    /// C's `*_DRAG`: moved while held.
    Drag,
    /// C's `*_RELEASE`: released.
    Release,
}

/// A cursor-control key (front ends translate arrow keys and the like).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorKey {
    /// `CURSOR_UP`.
    Up,
    /// `CURSOR_DOWN`.
    Down,
    /// `CURSOR_LEFT`.
    Left,
    /// `CURSOR_RIGHT`.
    Right,
    /// `CURSOR_SELECT` (the mid-end maps Enter to it).
    Select,
    /// `CURSOR_SELECT2` (the mid-end maps Space to it).
    Select2,
}

/// A menu action a front end sends as a key (C's `UI_*`). The mid-end
/// handles these itself; games never see them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiAction {
    /// `UI_QUIT`.
    Quit,
    /// `UI_NEWGAME`.
    NewGame,
    /// `UI_SOLVE`.
    Solve,
    /// `UI_UNDO`.
    Undo,
    /// `UI_REDO`.
    Redo,
}

/// The event itself, without modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// A key with a character code below `0x200`: printable ASCII, and
    /// control characters such as `'\b'`, `'\t'` and `'\x01'` (Ctrl+A,
    /// after the mid-end canonicalises it).
    Key(char),
    /// A mouse event.
    Mouse(MouseButton, MouseAction),
    /// A cursor key.
    Cursor(CursorKey),
    /// A menu action.
    Ui(UiAction),
}

bitflags! {
    /// Modifier flags (C's `MOD_*`), with C's bit values.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Modifiers: u32 {
        /// `MOD_STYLUS`: the event came from a stylus.
        const STYLUS = 0x0800;
        /// `MOD_CTRL`.
        const CTRL = 0x1000;
        /// `MOD_SHFT`.
        const SHIFT = 0x2000;
        /// `MOD_NUM_KEYPAD`: a digit typed on the numeric keypad.
        const NUM_KEYPAD = 0x4000;
    }
}

/// An input event for a game: what happened, and with which modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Button {
    /// The event.
    pub kind: ButtonKind,
    /// Modifier flags.
    pub mods: Modifiers,
}

/// C's `LEFT_BUTTON`: the first non-key code.
const FIRST_NON_KEY: u32 = 0x200;

/// Codes from `LEFT_BUTTON` on, in C's enum order. `UI_LOWER_BOUND` and
/// `UI_UPPER_BOUND` are markers, not events.
const NON_KEYS: [Option<ButtonKind>; 22] = {
    use ButtonKind::{Cursor, Mouse, Ui};
    use MouseAction::{Drag, Press, Release};
    use MouseButton::{Left, Middle, Right};
    [
        Some(Mouse(Left, Press)),
        Some(Mouse(Middle, Press)),
        Some(Mouse(Right, Press)),
        Some(Mouse(Left, Drag)),
        Some(Mouse(Middle, Drag)),
        Some(Mouse(Right, Drag)),
        Some(Mouse(Left, Release)),
        Some(Mouse(Middle, Release)),
        Some(Mouse(Right, Release)),
        Some(Cursor(CursorKey::Up)),
        Some(Cursor(CursorKey::Down)),
        Some(Cursor(CursorKey::Left)),
        Some(Cursor(CursorKey::Right)),
        Some(Cursor(CursorKey::Select)),
        Some(Cursor(CursorKey::Select2)),
        None, // UI_LOWER_BOUND
        Some(Ui(UiAction::Quit)),
        Some(Ui(UiAction::NewGame)),
        Some(Ui(UiAction::Solve)),
        Some(Ui(UiAction::Undo)),
        Some(Ui(UiAction::Redo)),
        None, // UI_UPPER_BOUND
    ]
};

impl Button {
    /// An event with no modifiers.
    #[must_use]
    pub const fn new(kind: ButtonKind) -> Self {
        Self {
            kind,
            mods: Modifiers::empty(),
        }
    }

    /// A key with no modifiers.
    #[must_use]
    pub const fn key(c: char) -> Self {
        Self::new(ButtonKind::Key(c))
    }

    /// A mouse event with no modifiers.
    #[must_use]
    pub const fn mouse(button: MouseButton, action: MouseAction) -> Self {
        Self::new(ButtonKind::Mouse(button, action))
    }

    /// A cursor key with no modifiers.
    #[must_use]
    pub const fn cursor(key: CursorKey) -> Self {
        Self::new(ButtonKind::Cursor(key))
    }

    /// The same event with `mods` added.
    #[must_use]
    pub const fn with(self, mods: Modifiers) -> Self {
        Self {
            kind: self.kind,
            mods: self.mods.union(mods),
        }
    }

    /// Decodes a C button code, or `None` for a code C doesn't define:
    /// a negative value, bits outside `MOD_MASK` and the event codes, the
    /// `UI_LOWER_BOUND`/`UI_UPPER_BOUND` markers, or anything between
    /// the last `UI_*` code and `MOD_STYLUS`.
    #[must_use]
    pub fn from_raw(raw: i32) -> Option<Self> {
        let raw = u32::try_from(raw).ok()?;
        let mods = Modifiers::from_bits(raw & Modifiers::all().bits())?;
        let code = raw & !Modifiers::all().bits();
        let kind = if code < FIRST_NON_KEY {
            ButtonKind::Key(char::from_u32(code)?)
        } else {
            let index = usize::try_from(code - FIRST_NON_KEY).ok()?;
            (*NON_KEYS.get(index)?)?
        };
        Some(Self { kind, mods })
    }

    /// The C button code, modifiers included.
    ///
    /// # Panics
    /// If the kind is a key above `'\u{1ff}'` (C can't represent it).
    #[must_use]
    pub fn to_raw(self) -> i32 {
        let code = match self.kind {
            ButtonKind::Key(c) => {
                let c = u32::from(c);
                assert!(c < FIRST_NON_KEY, "key {c:#x} has no C code");
                c
            }
            kind => {
                let index = NON_KEYS
                    .iter()
                    .position(|&k| k == Some(kind))
                    .expect("every non-key kind has a code");
                FIRST_NON_KEY + u32::try_from(index).expect("small index")
            }
        };
        i32::try_from(code | self.mods.bits()).expect("C codes fit in i32")
    }

    /// The mouse button and action, if this is a mouse event.
    #[must_use]
    pub fn mouse_event(self) -> Option<(MouseButton, MouseAction)> {
        match self.kind {
            ButtonKind::Mouse(button, action) => Some((button, action)),
            _ => None,
        }
    }

    /// C's `IS_MOUSE_DOWN`.
    #[must_use]
    pub fn is_mouse_down(self) -> bool {
        matches!(self.kind, ButtonKind::Mouse(_, MouseAction::Press))
    }

    /// C's `IS_MOUSE_DRAG`.
    #[must_use]
    pub fn is_mouse_drag(self) -> bool {
        matches!(self.kind, ButtonKind::Mouse(_, MouseAction::Drag))
    }

    /// C's `IS_MOUSE_RELEASE`.
    #[must_use]
    pub fn is_mouse_release(self) -> bool {
        matches!(self.kind, ButtonKind::Mouse(_, MouseAction::Release))
    }

    /// C's `IS_CURSOR_MOVE`: an arrow key.
    #[must_use]
    pub fn is_cursor_move(self) -> bool {
        matches!(
            self.kind,
            ButtonKind::Cursor(
                CursorKey::Up | CursorKey::Down | CursorKey::Left | CursorKey::Right
            )
        )
    }

    /// C's `IS_CURSOR_SELECT`.
    #[must_use]
    pub fn is_cursor_select(self) -> bool {
        matches!(
            self.kind,
            ButtonKind::Cursor(CursorKey::Select | CursorKey::Select2)
        )
    }

    /// A generic label for a button a game asks the front end to show
    /// (C's `button2label`): the character for a letter or digit, "Up",
    /// "Down", "Left", "Right", "Select" or "Clear" (backspace), or `None`
    /// where C calls `fatal("unknown generic key")`. Modifiers are
    /// ignored, as C compares the whole code (so any modifier gives
    /// `None`).
    #[doc(alias = "button2label")]
    #[must_use]
    pub fn label(self) -> Option<String> {
        if !self.mods.is_empty() {
            return None;
        }
        let label = match self.kind {
            ButtonKind::Key(c) if c.is_ascii_alphanumeric() => return Some(c.to_string()),
            ButtonKind::Key('\u{8}') => "Clear",
            ButtonKind::Cursor(CursorKey::Up) => "Up",
            ButtonKind::Cursor(CursorKey::Down) => "Down",
            ButtonKind::Cursor(CursorKey::Left) => "Left",
            ButtonKind::Cursor(CursorKey::Right) => "Right",
            ButtonKind::Cursor(CursorKey::Select) => "Select",
            _ => return None,
        };
        Some(label.to_owned())
    }
}

impl From<ButtonKind> for Button {
    fn from(kind: ButtonKind) -> Self {
        Self::new(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_codes_match_c_values() {
        // From puzzles.h: LEFT_BUTTON = 0x200 and the enum order after it.
        let cases = [
            (0x200, Button::mouse(MouseButton::Left, MouseAction::Press)),
            (0x202, Button::mouse(MouseButton::Right, MouseAction::Press)),
            (0x203, Button::mouse(MouseButton::Left, MouseAction::Drag)),
            (
                0x208,
                Button::mouse(MouseButton::Right, MouseAction::Release),
            ),
            (0x209, Button::cursor(CursorKey::Up)),
            (0x20e, Button::cursor(CursorKey::Select2)),
            (0x210, Button::new(ButtonKind::Ui(UiAction::Quit))),
            (0x214, Button::new(ButtonKind::Ui(UiAction::Redo))),
            (i32::from(b'a'), Button::key('a')),
            (0x1000 | 0x09, Button::key('\t').with(Modifiers::CTRL)),
            (
                0x4000 | i32::from(b'7'),
                Button::key('7').with(Modifiers::NUM_KEYPAD),
            ),
        ];
        for (raw, button) in cases {
            assert_eq!(Button::from_raw(raw), Some(button), "{raw:#x}");
            assert_eq!(button.to_raw(), raw, "{button:?}");
        }
    }

    #[test]
    fn every_defined_code_round_trips() {
        let mut defined = 0;
        for raw in 0..0x8000 {
            if let Some(button) = Button::from_raw(raw) {
                assert_eq!(button.to_raw(), raw);
                defined += 1;
            }
        }
        // (0x200 keys + 20 events) x 16 modifier combinations.
        assert_eq!(defined, (0x200 + 20) * 16);
    }

    #[test]
    fn undefined_codes_are_rejected() {
        for raw in [-1, 0x20f, 0x215, 0x7ff, 0x8000, 0x10000] {
            assert_eq!(Button::from_raw(raw), None, "{raw:#x}");
        }
    }

    #[test]
    fn predicates() {
        let press = Button::mouse(MouseButton::Middle, MouseAction::Press);
        assert!(press.is_mouse_down() && !press.is_mouse_drag() && !press.is_mouse_release());
        assert!(Button::cursor(CursorKey::Left).is_cursor_move());
        assert!(!Button::cursor(CursorKey::Select).is_cursor_move());
        assert!(Button::cursor(CursorKey::Select2).is_cursor_select());
        assert!(!Button::key('a').is_cursor_select());
    }

    #[test]
    fn labels() {
        assert_eq!(Button::key('q').label().as_deref(), Some("q"));
        assert_eq!(Button::key('7').label().as_deref(), Some("7"));
        assert_eq!(Button::key('\u{8}').label().as_deref(), Some("Clear"));
        assert_eq!(
            Button::cursor(CursorKey::Select).label().as_deref(),
            Some("Select")
        );
        assert_eq!(Button::cursor(CursorKey::Select2).label(), None);
        assert_eq!(Button::key('+').label(), None);
        assert_eq!(Button::key('a').with(Modifiers::SHIFT).label(), None);
    }
}

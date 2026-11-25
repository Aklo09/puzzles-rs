//! Bindings to `drawing.c` and the drawing helpers in `misc.c`, through
//! the recording drawing API in `csrc/recorder.c`; plus the button macros
//! and button helpers of `puzzles.h` / `misc.c`.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;

use super::sfree;

/// Opaque C `drawing`.
#[repr(C)]
pub(super) struct DrawingC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// Opaque C `blitter`.
#[repr(C)]
struct BlitterC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    fn oracle_recorder_new() -> *mut DrawingC;
    fn oracle_recorder_text(dr: *mut DrawingC) -> *const c_char;
    fn oracle_recorder_free(dr: *mut DrawingC);

    fn draw_text(
        dr: *mut DrawingC,
        x: c_int,
        y: c_int,
        fonttype: c_int,
        fontsize: c_int,
        align: c_int,
        colour: c_int,
        text: *const c_char,
    );
    fn draw_rect(dr: *mut DrawingC, x: c_int, y: c_int, w: c_int, h: c_int, colour: c_int);
    fn draw_line(dr: *mut DrawingC, x1: c_int, y1: c_int, x2: c_int, y2: c_int, colour: c_int);
    fn draw_polygon(
        dr: *mut DrawingC,
        coords: *const c_int,
        npoints: c_int,
        fillcolour: c_int,
        outlinecolour: c_int,
    );
    fn draw_circle(
        dr: *mut DrawingC,
        cx: c_int,
        cy: c_int,
        radius: c_int,
        fillcolour: c_int,
        outlinecolour: c_int,
    );
    fn draw_thick_line(
        dr: *mut DrawingC,
        thickness: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        colour: c_int,
    );
    fn draw_update(dr: *mut DrawingC, x: c_int, y: c_int, w: c_int, h: c_int);
    fn clip(dr: *mut DrawingC, x: c_int, y: c_int, w: c_int, h: c_int);
    fn unclip(dr: *mut DrawingC);
    fn start_draw(dr: *mut DrawingC);
    fn end_draw(dr: *mut DrawingC);
    fn blitter_new(dr: *mut DrawingC, w: c_int, h: c_int) -> *mut BlitterC;
    fn blitter_free(dr: *mut DrawingC, bl: *mut BlitterC);
    fn blitter_save(dr: *mut DrawingC, bl: *mut BlitterC, x: c_int, y: c_int);
    fn blitter_load(dr: *mut DrawingC, bl: *mut BlitterC, x: c_int, y: c_int);
    fn text_fallback(dr: *mut DrawingC, strings: *const *const c_char, n: c_int) -> *mut c_char;

    fn draw_rect_outline(dr: *mut DrawingC, x: c_int, y: c_int, w: c_int, h: c_int, colour: c_int);
    fn draw_rect_corners(dr: *mut DrawingC, cx: c_int, cy: c_int, r: c_int, col: c_int);
    fn draw_text_outline(
        dr: *mut DrawingC,
        x: c_int,
        y: c_int,
        fonttype: c_int,
        fontsize: c_int,
        align: c_int,
        text_colour: c_int,
        outline_colour: c_int,
        text: *const c_char,
    );

    fn oracle_button_class(button: c_int) -> c_int;
    fn oracle_move_cursor(
        button: c_int,
        x: *mut c_int,
        y: *mut c_int,
        maxw: c_int,
        maxh: c_int,
        wrap: bool,
        visible: *mut bool,
    ) -> c_int;
    fn button2label(button: c_int) -> *mut c_char;
}

/// A C `drawing` over the recording drawing API: every call through it
/// is logged as text (see [`trace`](Self::trace)).
///
/// The methods call C's `drawing.c` functions and `misc.c` helpers with
/// the same arguments as the C prototypes (colours as ints, -1 for none).
pub struct CRecorder {
    dr: NonNull<DrawingC>,
    blitters: Vec<NonNull<BlitterC>>,
}

impl Default for CRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl CRecorder {
    /// A fresh recorder with an empty trace.
    ///
    /// # Panics
    /// If C returns NULL (smalloc aborts first).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: returns a fresh drawing (smalloc never returns NULL).
        let dr = unsafe { oracle_recorder_new() };
        Self {
            dr: NonNull::new(dr).expect("recorder allocation failed"),
            blitters: Vec::new(),
        }
    }

    /// Everything recorded so far, one line per call.
    ///
    /// # Panics
    /// If the trace isn't UTF-8 (it holds only what callers passed).
    #[must_use]
    pub fn trace(&self) -> String {
        // SAFETY: the recorder keeps a NUL-terminated buffer while alive.
        unsafe { CStr::from_ptr(oracle_recorder_text(self.dr.as_ptr())) }
            .to_str()
            .expect("trace is UTF-8")
            .to_owned()
    }

    /// C's `draw_text`.
    ///
    /// # Panics
    /// If `text` contains a NUL byte.
    #[allow(clippy::too_many_arguments, reason = "mirrors C")]
    pub fn text(
        &mut self,
        x: i32,
        y: i32,
        font: i32,
        size: i32,
        align: i32,
        colour: i32,
        text: &str,
    ) {
        let text = CString::new(text).expect("no NUL in text");
        // SAFETY: the drawing is live; `text` is a valid C string.
        unsafe {
            draw_text(
                self.dr.as_ptr(),
                x,
                y,
                font,
                size,
                align,
                colour,
                text.as_ptr(),
            );
        }
    }

    /// C's `draw_rect`.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_rect(self.dr.as_ptr(), x, y, w, h, colour) }
    }

    /// C's `draw_line`.
    pub fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_line(self.dr.as_ptr(), x1, y1, x2, y2, colour) }
    }

    /// C's `draw_polygon`.
    ///
    /// # Panics
    /// If there are more points than fit in a C int.
    pub fn polygon(&mut self, points: &[(i32, i32)], fill: i32, outline: i32) {
        let coords: Vec<c_int> = points.iter().flat_map(|&(x, y)| [x, y]).collect();
        let n = c_int::try_from(points.len()).expect("too many points");
        // SAFETY: the drawing is live; `coords` holds 2 * n ints.
        unsafe { draw_polygon(self.dr.as_ptr(), coords.as_ptr(), n, fill, outline) }
    }

    /// C's `draw_circle`.
    pub fn circle(&mut self, cx: i32, cy: i32, radius: i32, fill: i32, outline: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_circle(self.dr.as_ptr(), cx, cy, radius, fill, outline) }
    }

    /// C's `draw_thick_line`.
    pub fn thick_line(&mut self, thickness: f32, x1: f32, y1: f32, x2: f32, y2: f32, colour: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_thick_line(self.dr.as_ptr(), thickness, x1, y1, x2, y2, colour) }
    }

    /// C's `draw_update`.
    pub fn update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_update(self.dr.as_ptr(), x, y, w, h) }
    }

    /// C's `clip`.
    pub fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        // SAFETY: the drawing is live.
        unsafe { clip(self.dr.as_ptr(), x, y, w, h) }
    }

    /// C's `unclip`.
    pub fn unclip(&mut self) {
        // SAFETY: the drawing is live.
        unsafe { unclip(self.dr.as_ptr()) }
    }

    /// C's `start_draw`.
    pub fn start_draw(&mut self) {
        // SAFETY: the drawing is live.
        unsafe { start_draw(self.dr.as_ptr()) }
    }

    /// C's `end_draw`.
    pub fn end_draw(&mut self) {
        // SAFETY: the drawing is live.
        unsafe { end_draw(self.dr.as_ptr()) }
    }

    /// C's `blitter_new`: returns an index for [`blitter_save`] and
    /// [`blitter_load`](Self::blitter_load).
    ///
    /// [`blitter_save`]: Self::blitter_save
    ///
    /// # Panics
    /// If C returns NULL (smalloc aborts first).
    pub fn blitter_new(&mut self, w: i32, h: i32) -> usize {
        // SAFETY: the drawing is live; returns a fresh blitter.
        let bl = unsafe { blitter_new(self.dr.as_ptr(), w, h) };
        self.blitters
            .push(NonNull::new(bl).expect("blitter allocation failed"));
        self.blitters.len() - 1
    }

    /// C's `blitter_save`.
    ///
    /// # Panics
    /// If `blitter` isn't an index from [`blitter_new`](Self::blitter_new).
    pub fn blitter_save(&mut self, blitter: usize, x: i32, y: i32) {
        let bl = self.blitters[blitter];
        // SAFETY: the drawing and blitter are live.
        unsafe { blitter_save(self.dr.as_ptr(), bl.as_ptr(), x, y) }
    }

    /// C's `blitter_load`.
    ///
    /// # Panics
    /// If `blitter` isn't an index from [`blitter_new`](Self::blitter_new).
    pub fn blitter_load(&mut self, blitter: usize, x: i32, y: i32) {
        let bl = self.blitters[blitter];
        // SAFETY: the drawing and blitter are live.
        unsafe { blitter_load(self.dr.as_ptr(), bl.as_ptr(), x, y) }
    }

    /// C's `text_fallback`: the index of the string it picked.
    ///
    /// # Panics
    /// If no string is plain ASCII (C asserts), or one contains a NUL.
    #[must_use]
    pub fn text_fallback(&self, strings: &[&str]) -> usize {
        assert!(strings.iter().any(|s| s.is_ascii()), "C would assert");
        let owned: Vec<CString> = strings
            .iter()
            .map(|s| CString::new(*s).expect("no NUL"))
            .collect();
        let ptrs: Vec<*const c_char> = owned.iter().map(|s| s.as_ptr()).collect();
        let n = c_int::try_from(ptrs.len()).expect("too many strings");
        // SAFETY: the drawing is live; `ptrs` holds n valid C strings.
        // Returns a fresh copy of the chosen one.
        let chosen = unsafe { text_fallback(self.dr.as_ptr(), ptrs.as_ptr(), n) };
        // SAFETY: `chosen` is a valid C string until freed below.
        let text = unsafe { CStr::from_ptr(chosen) }.to_owned();
        // SAFETY: allocated by C's dupstr, freed once.
        unsafe { sfree(chosen.cast::<c_void>()) };
        owned
            .iter()
            .position(|s| *s == text)
            .expect("C returned one of the strings")
    }

    /// C's `draw_rect_outline`.
    pub fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, colour: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_rect_outline(self.dr.as_ptr(), x, y, w, h, colour) }
    }

    /// C's `draw_rect_corners`.
    pub fn rect_corners(&mut self, cx: i32, cy: i32, r: i32, colour: i32) {
        // SAFETY: the drawing is live.
        unsafe { draw_rect_corners(self.dr.as_ptr(), cx, cy, r, colour) }
    }

    /// C's `draw_text_outline` (`outline` -1 for none).
    ///
    /// # Panics
    /// If `text` contains a NUL byte.
    #[allow(clippy::too_many_arguments, reason = "mirrors C")]
    pub fn text_outline(
        &mut self,
        x: i32,
        y: i32,
        font: i32,
        size: i32,
        align: i32,
        colour: i32,
        outline: i32,
        text: &str,
    ) {
        let text = CString::new(text).expect("no NUL in text");
        // SAFETY: the drawing is live; `text` is a valid C string.
        unsafe {
            draw_text_outline(
                self.dr.as_ptr(),
                x,
                y,
                font,
                size,
                align,
                colour,
                outline,
                text.as_ptr(),
            );
        }
    }
}

impl Drop for CRecorder {
    fn drop(&mut self) {
        for bl in self.blitters.drain(..) {
            // SAFETY: each blitter came from blitter_new and is freed once.
            unsafe { blitter_free(self.dr.as_ptr(), bl.as_ptr()) };
        }
        // SAFETY: the recorder came from oracle_recorder_new, freed once.
        unsafe { oracle_recorder_free(self.dr.as_ptr()) };
    }
}

/// C's button macros for `button`, as a bit set: 1 `IS_MOUSE_DOWN`,
/// 2 `IS_MOUSE_DRAG`, 4 `IS_MOUSE_RELEASE`, 8 `IS_CURSOR_MOVE`,
/// 16 `IS_CURSOR_SELECT`, 32 `IS_UI_FAKE_KEY`.
#[must_use]
pub fn c_button_class(button: i32) -> i32 {
    // SAFETY: a pure function of an int.
    unsafe { oracle_button_class(button) }
}

/// C's `move_cursor`: 0 for `MOVE_UNUSED`, 1 `MOVE_NO_EFFECT`,
/// 2 `MOVE_UI_UPDATE`, with the updated position and visibility.
///
/// # Panics
/// If `wrap` is set with an empty grid (C would divide by zero).
#[must_use]
pub fn c_move_cursor(
    button: i32,
    (x, y): (i32, i32),
    (max_w, max_h): (i32, i32),
    wrap: bool,
    visible: Option<bool>,
) -> (i32, (i32, i32), Option<bool>) {
    assert!(
        !wrap || (max_w != 0 && max_h != 0),
        "C would divide by zero"
    );
    let (mut x, mut y) = (x, y);
    let mut vis = visible.unwrap_or(false);
    let vis_ptr: *mut bool = if visible.is_some() {
        &raw mut vis
    } else {
        std::ptr::null_mut()
    };
    // SAFETY: x, y and (if non-NULL) vis_ptr point at live locals.
    let ret =
        unsafe { oracle_move_cursor(button, &raw mut x, &raw mut y, max_w, max_h, wrap, vis_ptr) };
    (ret, (x, y), visible.map(|_| vis))
}

/// C's `button2label`, for buttons it has a label for.
///
/// # Panics
/// If C returns NULL. Callers must only pass buttons with a label: for
/// others C calls `fatal`, which aborts the process.
#[must_use]
pub fn c_button2label(button: i32) -> String {
    // SAFETY: the caller only passes buttons C labels (else C aborts).
    let label = unsafe { button2label(button) };
    assert!(!label.is_null(), "button2label returned NULL");
    // SAFETY: a valid C string until freed below.
    let text = unsafe { CStr::from_ptr(label) }
        .to_str()
        .expect("ASCII label")
        .to_owned();
    // SAFETY: allocated by dupstr, freed once.
    unsafe { sfree(label.cast::<c_void>()) };
    text
}

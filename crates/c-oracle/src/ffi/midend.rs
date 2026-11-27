//! Bindings to `midend.c` through `csrc/midend_harness.c`, a minimal C
//! front end: the mid-end of a C game, driven with text in and out.

use std::ffi::{CString, c_char, c_int};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::{self, NonNull};

use super::game::{CGame, GameC, borrowed, c_string, owned};
use super::sfree;

#[repr(C)]
struct MidendC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    fn oracle_midend_new(
        g: *const GameC,
        seed: *const c_char,
        seedlen: c_int,
        interactive: bool,
    ) -> *mut MidendC;
    fn oracle_midend_free(om: *mut MidendC);
    fn oracle_midend_size(
        om: *mut MidendC,
        x: *mut c_int,
        y: *mut c_int,
        user_size: bool,
        dpr: f64,
    );
    fn oracle_midend_tilesize(om: *mut MidendC) -> c_int;
    fn oracle_midend_new_game(om: *mut MidendC);
    fn oracle_midend_restart_game(om: *mut MidendC);
    fn oracle_midend_process_key(om: *mut MidendC, x: c_int, y: c_int, button: c_int) -> c_int;
    fn oracle_midend_timer(om: *mut MidendC, tplus: f32);
    fn oracle_midend_stop_anim(om: *mut MidendC);
    fn oracle_midend_wants_timer(om: *mut MidendC) -> bool;
    fn oracle_midend_redraw(om: *mut MidendC);
    fn oracle_midend_force_redraw(om: *mut MidendC) -> *const c_char;
    fn oracle_midend_status_text(om: *mut MidendC) -> *const c_char;
    fn oracle_midend_solve(om: *mut MidendC) -> *const c_char;
    fn oracle_midend_status(om: *mut MidendC) -> c_int;
    fn oracle_midend_can_undo(om: *mut MidendC) -> bool;
    fn oracle_midend_can_redo(om: *mut MidendC) -> bool;
    fn oracle_midend_text_format(om: *mut MidendC) -> *mut c_char;
    fn oracle_midend_game_id(om: *mut MidendC, id: *const c_char) -> *const c_char;
    fn oracle_midend_get_game_id(om: *mut MidendC) -> *mut c_char;
    fn oracle_midend_get_random_seed(om: *mut MidendC) -> *mut c_char;
    fn oracle_midend_set_params(om: *mut MidendC, params: *const c_char);
    fn oracle_midend_presets(om: *mut MidendC) -> c_int;
    fn oracle_midend_set_preset(om: *mut MidendC, id: c_int) -> bool;
    fn oracle_midend_which_preset(om: *mut MidendC) -> c_int;
    fn oracle_midend_get_config(om: *mut MidendC, which: c_int) -> *mut c_char;
    fn oracle_midend_set_config(
        om: *mut MidendC,
        which: c_int,
        values: *const *const c_char,
        n: c_int,
    ) -> *const c_char;
    fn oracle_midend_serialise(om: *mut MidendC, len: *mut c_int) -> *mut c_char;
    fn oracle_midend_deserialise(
        om: *mut MidendC,
        data: *const c_char,
        len: c_int,
    ) -> *const c_char;
    fn oracle_identify_game(
        data: *const c_char,
        len: c_int,
        name: *mut *mut c_char,
    ) -> *const c_char;
    fn oracle_midend_save_prefs(om: *mut MidendC, len: *mut c_int) -> *mut c_char;
    fn oracle_midend_load_prefs(om: *mut MidendC, data: *const c_char, len: c_int)
    -> *const c_char;
    fn oracle_midend_cursor(
        om: *mut MidendC,
        x: *mut c_int,
        y: *mut c_int,
        w: *mut c_int,
        h: *mut c_int,
    ) -> bool;
    fn oracle_format_g(x: f64, out: *mut c_char, size: c_int);
    fn oracle_atof(s: *const c_char) -> f64;
}

/// Copies and frees `len` bytes C allocated.
///
/// # Safety
/// `p` points to at least `len` bytes from smalloc.
unsafe fn owned_bytes(p: *mut c_char, len: c_int) -> Vec<u8> {
    let len = usize::try_from(len).expect("non-negative length");
    // SAFETY: as the caller promises.
    let bytes = unsafe { std::slice::from_raw_parts(p.cast::<u8>(), len) }.to_vec();
    // SAFETY: allocated by smalloc and not used again.
    unsafe { sfree(p.cast()) };
    bytes
}

fn err(p: *const c_char) -> Result<(), String> {
    // SAFETY: NULL or a static C string.
    unsafe { borrowed(p) }.map_or(Ok(()), Err)
}

/// C's `printf("%g", x)`.
///
/// # Panics
/// Never (the output always fits).
#[must_use]
pub fn c_format_g(x: f64) -> String {
    let mut buf = [0 as c_char; 64];
    // SAFETY: valid buffer of the given size.
    unsafe { oracle_format_g(x, buf.as_mut_ptr(), 64) };
    // SAFETY: snprintf NUL-terminated it.
    unsafe { borrowed(buf.as_ptr()) }.expect("a string")
}

/// C's `atof`.
///
/// # Panics
/// If `s` contains a NUL byte.
#[must_use]
pub fn c_atof(s: &[u8]) -> f64 {
    let s = CString::new(s).expect("no NUL");
    // SAFETY: valid C string.
    unsafe { oracle_atof(s.as_ptr()) }
}

/// C's `identify_game` on a buffer.
///
/// # Errors
/// C's message.
///
/// # Panics
/// If the data is huge.
pub fn c_identify_game(data: &[u8]) -> Result<String, String> {
    let len = c_int::try_from(data.len()).expect("fits in an int");
    let mut name = ptr::null_mut();
    // SAFETY: valid buffer; returns a static error or a fresh name.
    let e = unsafe { oracle_identify_game(data.as_ptr().cast(), len, &raw mut name) };
    err(e)?;
    // SAFETY: a fresh name on success.
    Ok(unsafe { owned(name) }.unwrap_or_default())
}

/// A C mid-end with a recording drawing (see `csrc/midend_harness.c`).
pub struct CMidend {
    om: NonNull<MidendC>,
}

impl Drop for CMidend {
    fn drop(&mut self) {
        // SAFETY: created by oracle_midend_new, freed once.
        unsafe { oracle_midend_free(self.om.as_ptr()) }
    }
}

impl CMidend {
    /// A mid-end for `game` whose seed generator starts from `seed`.
    ///
    /// # Panics
    /// If the seed is huge.
    #[must_use]
    pub fn new(game: CGame, seed: &[u8], interactive: bool) -> Self {
        let len = c_int::try_from(seed.len()).expect("fits in an int");
        // SAFETY: valid game and seed buffer (copied during the call).
        let om = unsafe { oracle_midend_new(game.ptr(), seed.as_ptr().cast(), len, interactive) };
        Self {
            om: NonNull::new(om).expect("allocation failed"),
        }
    }

    fn p(&self) -> *mut MidendC {
        self.om.as_ptr()
    }

    /// `midend_size`.
    pub fn size(&mut self, w: i32, h: i32, user_size: bool, dpr: f64) -> (i32, i32) {
        let (mut x, mut y) = (w, h);
        // SAFETY: live mid-end, valid pointers.
        unsafe { oracle_midend_size(self.p(), &raw mut x, &raw mut y, user_size, dpr) };
        (x, y)
    }

    /// `midend_tilesize`.
    pub fn tilesize(&mut self) -> i32 {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_tilesize(self.p()) }
    }

    /// `midend_new_game`.
    pub fn new_game(&mut self) {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_new_game(self.p()) }
    }

    /// `midend_restart_game`.
    pub fn restart_game(&mut self) {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_restart_game(self.p()) }
    }

    /// `midend_process_key`: C's `PKR_*` code.
    pub fn process_key(&mut self, x: i32, y: i32, button: i32) -> i32 {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_process_key(self.p(), x, y, button) }
    }

    /// `midend_timer`.
    pub fn timer(&mut self, tplus: f32) {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_timer(self.p(), tplus) }
    }

    /// `midend_stop_anim`.
    pub fn stop_anim(&mut self) {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_stop_anim(self.p()) }
    }

    /// Whether the front end's timer is on.
    pub fn wants_timer(&mut self) -> bool {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_wants_timer(self.p()) }
    }

    /// `midend_redraw`, as a front end does after each event.
    pub fn redraw(&mut self) {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_redraw(self.p()) }
    }

    /// `midend_force_redraw`: its draw trace.
    ///
    /// # Panics
    /// If C returns no trace.
    pub fn force_redraw(&mut self) -> String {
        // SAFETY: live mid-end; the trace is copied at once.
        unsafe { borrowed(oracle_midend_force_redraw(self.p())) }.expect("a trace")
    }

    /// The last status bar text the front end got.
    pub fn status_text(&mut self) -> Option<String> {
        // SAFETY: live mid-end; copied at once.
        unsafe { borrowed(oracle_midend_status_text(self.p())) }
    }

    /// `midend_solve`.
    ///
    /// # Errors
    /// C's message.
    pub fn solve(&mut self) -> Result<(), String> {
        // SAFETY: live mid-end.
        err(unsafe { oracle_midend_solve(self.p()) })
    }

    /// `midend_status`.
    pub fn status(&mut self) -> i32 {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_status(self.p()) }
    }

    /// `midend_can_undo` and `midend_can_redo`.
    pub fn can_undo_redo(&mut self) -> (bool, bool) {
        // SAFETY: live mid-end.
        unsafe {
            (
                oracle_midend_can_undo(self.p()),
                oracle_midend_can_redo(self.p()),
            )
        }
    }

    /// `midend_text_format`.
    pub fn text_format(&mut self) -> Option<String> {
        // SAFETY: live mid-end; NULL or fresh.
        unsafe { owned(oracle_midend_text_format(self.p())) }
    }

    /// `midend_game_id`.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If `id` contains a NUL byte.
    pub fn game_id(&mut self, id: &str) -> Result<(), String> {
        let id = c_string(id);
        // SAFETY: live mid-end, valid string.
        err(unsafe { oracle_midend_game_id(self.p(), id.as_ptr()) })
    }

    /// `midend_get_game_id` (only with a game).
    pub fn get_game_id(&mut self) -> Option<String> {
        // SAFETY: live mid-end; fresh string.
        unsafe { owned(oracle_midend_get_game_id(self.p())) }
    }

    /// `midend_get_random_seed`.
    pub fn get_random_seed(&mut self) -> Option<String> {
        // SAFETY: live mid-end; NULL or fresh.
        unsafe { owned(oracle_midend_get_random_seed(self.p())) }
    }

    /// `midend_set_params` with parameters decoded over the defaults.
    ///
    /// # Panics
    /// If `params` contains a NUL byte.
    pub fn set_params(&mut self, params: &str) {
        let params = c_string(params);
        // SAFETY: live mid-end, valid string.
        unsafe { oracle_midend_set_params(self.p(), params.as_ptr()) }
    }

    /// Builds the preset menu: the number of ids.
    pub fn presets(&mut self) -> i32 {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_presets(self.p()) }
    }

    /// Chooses a preset by id; false if it isn't one.
    pub fn set_preset(&mut self, id: i32) -> bool {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_set_preset(self.p(), id) }
    }

    /// `midend_which_preset` (-1 for none).
    pub fn which_preset(&mut self) -> i32 {
        // SAFETY: live mid-end.
        unsafe { oracle_midend_which_preset(self.p()) }
    }

    /// `midend_get_config` as text (see the harness).
    pub fn get_config(&mut self, which: i32) -> Option<String> {
        // SAFETY: live mid-end; NULL or fresh.
        unsafe { owned(oracle_midend_get_config(self.p(), which)) }
    }

    /// `midend_set_config` with the dialog's items replaced in order.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If a value contains a NUL byte.
    pub fn set_config(&mut self, which: i32, values: &[&str]) -> Result<(), String> {
        let values: Vec<CString> = values.iter().map(|v| c_string(v)).collect();
        let ptrs: Vec<*const c_char> = values.iter().map(|v| v.as_ptr()).collect();
        let n = c_int::try_from(ptrs.len()).expect("few values");
        // SAFETY: live mid-end, n valid strings.
        err(unsafe { oracle_midend_set_config(self.p(), which, ptrs.as_ptr(), n) })
    }

    /// `midend_serialise`.
    pub fn serialise(&mut self) -> Vec<u8> {
        let mut len = 0;
        // SAFETY: live mid-end; a fresh buffer of `len` bytes.
        unsafe { owned_bytes(oracle_midend_serialise(self.p(), &raw mut len), len) }
    }

    /// `midend_deserialise`.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If the data is huge.
    pub fn deserialise(&mut self, data: &[u8]) -> Result<(), String> {
        let len = c_int::try_from(data.len()).expect("fits in an int");
        // SAFETY: live mid-end, valid buffer.
        err(unsafe { oracle_midend_deserialise(self.p(), data.as_ptr().cast(), len) })
    }

    /// `midend_save_prefs`.
    pub fn save_prefs(&mut self) -> Vec<u8> {
        let mut len = 0;
        // SAFETY: live mid-end; a fresh buffer of `len` bytes.
        unsafe { owned_bytes(oracle_midend_save_prefs(self.p(), &raw mut len), len) }
    }

    /// `midend_load_prefs`.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If the data is huge.
    pub fn load_prefs(&mut self, data: &[u8]) -> Result<(), String> {
        let len = c_int::try_from(data.len()).expect("fits in an int");
        // SAFETY: live mid-end, valid buffer.
        err(unsafe { oracle_midend_load_prefs(self.p(), data.as_ptr().cast(), len) })
    }

    /// `midend_get_cursor_location`.
    pub fn cursor(&mut self) -> Option<(i32, i32, i32, i32)> {
        let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
        // SAFETY: live mid-end, valid pointers.
        let shown = unsafe {
            oracle_midend_cursor(self.p(), &raw mut x, &raw mut y, &raw mut w, &raw mut h)
        };
        shown.then_some((x, y, w, h))
    }
}

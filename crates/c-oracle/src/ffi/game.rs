//! Bindings to C games (`const game` structs compiled with `COMBINED`)
//! through `csrc/game_harness.c`, which drives a game one back-end call
//! at a time as the mid-end does.

use std::ffi::{CStr, CString, c_char, c_int};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::{self, NonNull};

use super::sfree;

/// Opaque C `struct game`.
#[repr(C)]
pub(super) struct GameC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// Opaque harness session.
#[repr(C)]
struct SessionC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    static fifteen: GameC;
    static flip: GameC;
    static guess: GameC;
    static mosaic: GameC;
    static nullgame: GameC;

    fn oracle_game_colours(g: *const GameC, out: *mut f32, max: c_int) -> c_int;
    fn oracle_mkhighlight(base: *const f32, out: *mut f32);

    fn oracle_encode_params(g: *const GameC, params: *const c_char, full: bool) -> *mut c_char;
    fn oracle_validate_params(g: *const GameC, params: *const c_char, full: bool) -> *const c_char;
    fn oracle_preset(g: *const GameC, i: c_int, name: *mut *mut c_char) -> *mut c_char;
    fn oracle_new_desc(
        g: *const GameC,
        params: *const c_char,
        seed: *const c_char,
        seedlen: c_int,
        interactive: bool,
        aux: *mut *mut c_char,
    ) -> *mut c_char;
    fn oracle_validate_desc(
        g: *const GameC,
        params: *const c_char,
        desc: *const c_char,
    ) -> *const c_char;

    fn oracle_session_new(
        g: *const GameC,
        params: *const c_char,
        desc: *const c_char,
        tilesize: c_int,
    ) -> *mut SessionC;
    fn oracle_session_free(s: *mut SessionC);
    fn oracle_session_interpret(
        s: *mut SessionC,
        x: c_int,
        y: c_int,
        button: c_int,
        mv: *mut *mut c_char,
    ) -> c_int;
    fn oracle_session_execute(s: *mut SessionC, mv: *const c_char) -> bool;
    fn oracle_session_anim_length(s: *mut SessionC, dir: c_int) -> f32;
    fn oracle_session_flash_length(s: *mut SessionC, dir: c_int) -> f32;
    fn oracle_session_redraw(
        s: *mut SessionC,
        animate: bool,
        dir: c_int,
        anim_time: f32,
        flash_time: f32,
        status: *mut *const c_char,
    ) -> *const c_char;
    fn oracle_session_solve(
        s: *mut SessionC,
        aux: *const c_char,
        error: *mut *const c_char,
    ) -> *mut c_char;
    fn oracle_session_text_format(s: *mut SessionC) -> *mut c_char;
    fn oracle_session_status(s: *mut SessionC) -> c_int;
    fn oracle_session_encode_ui(s: *mut SessionC) -> *mut c_char;
    fn oracle_session_key_label(s: *mut SessionC, button: c_int) -> *const c_char;
    fn oracle_session_cursor(
        s: *mut SessionC,
        x: *mut c_int,
        y: *mut c_int,
        w: *mut c_int,
        h: *mut c_int,
    ) -> bool;
    fn oracle_session_size(s: *mut SessionC, tilesize: c_int, x: *mut c_int, y: *mut c_int);
}

pub(super) fn c_string(s: &str) -> CString {
    CString::new(s).expect("no NUL in strings passed to C")
}

/// Copies a C string the caller doesn't own; `None` for NULL.
///
/// # Safety
/// `p` is NULL or a valid NUL-terminated string.
pub(super) unsafe fn borrowed(p: *const c_char) -> Option<String> {
    // SAFETY: as the caller promises.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// Copies and frees a string C allocated; `None` for NULL.
///
/// # Safety
/// `p` is NULL or a valid NUL-terminated string from smalloc.
pub(super) unsafe fn owned(p: *mut c_char) -> Option<String> {
    // SAFETY: as the caller promises.
    let s = unsafe { borrowed(p) };
    if !p.is_null() {
        // SAFETY: allocated by smalloc and not used again.
        unsafe { sfree(p.cast()) };
    }
    s
}

/// A C game, by its `struct game`.
#[derive(Clone, Copy, Debug)]
pub struct CGame(&'static GameC);

impl std::fmt::Debug for GameC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GameC")
    }
}

impl CGame {
    /// `fifteen.c`.
    #[must_use]
    pub fn fifteen() -> Self {
        // SAFETY: a constant struct defined by fifteen.c.
        Self(unsafe { &fifteen })
    }

    /// `flip.c`.
    #[must_use]
    pub fn flip() -> Self {
        // SAFETY: a constant struct defined by flip.c.
        Self(unsafe { &flip })
    }

    /// `guess.c`.
    #[must_use]
    pub fn guess() -> Self {
        // SAFETY: a constant struct defined by guess.c.
        Self(unsafe { &guess })
    }

    /// `mosaic.c`.
    #[must_use]
    pub fn mosaic() -> Self {
        // SAFETY: a constant struct defined by mosaic.c.
        Self(unsafe { &mosaic })
    }

    /// `nullgame.c`.
    #[must_use]
    pub fn nullgame() -> Self {
        // SAFETY: a constant struct defined by nullgame.c.
        Self(unsafe { &nullgame })
    }

    pub(super) fn ptr(self) -> *const GameC {
        self.0
    }

    /// The game's `colours()`, with the oracle's default background (0.9
    /// grey): one RGB triple per palette entry.
    ///
    /// # Panics
    /// If the game has more than 256 colours.
    #[must_use]
    pub fn colours(self) -> Vec<[f32; 3]> {
        const MAX: usize = 256;
        let mut out = vec![0.0_f32; MAX * 3];
        // SAFETY: valid game; `out` holds MAX triples.
        let n = unsafe { oracle_game_colours(self.ptr(), out.as_mut_ptr(), MAX as c_int) };
        let n = usize::try_from(n).expect("a count");
        assert!(n <= MAX, "too many colours");
        out.as_chunks::<3>().0[..n].to_vec()
    }

    /// `encode_params(decode_params(default_params(), params), full)`.
    ///
    /// # Panics
    /// If `params` contains a NUL byte.
    #[must_use]
    pub fn encode_params(self, params: &str, full: bool) -> String {
        let params = c_string(params);
        // SAFETY: valid game and string; returns a fresh string.
        unsafe { owned(oracle_encode_params(self.ptr(), params.as_ptr(), full)) }
            .expect("encode_params returns a string")
    }

    /// `validate_params` of the decoded parameters.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If `params` contains a NUL byte.
    pub fn validate_params(self, params: &str, full: bool) -> Result<(), String> {
        let params = c_string(params);
        // SAFETY: valid game and string; returns NULL or a static string.
        let err = unsafe { borrowed(oracle_validate_params(self.ptr(), params.as_ptr(), full)) };
        err.map_or(Ok(()), Err)
    }

    /// The presets from `fetch_preset`: name and full parameter
    /// encoding.
    ///
    /// # Panics
    /// If C returns a preset without a name.
    #[must_use]
    pub fn presets(self) -> Vec<(String, String)> {
        let mut presets = Vec::new();
        for i in 0.. {
            let mut name = ptr::null_mut();
            // SAFETY: valid game; returns NULL or fresh strings.
            let Some(params) = (unsafe { owned(oracle_preset(self.ptr(), i, &raw mut name)) })
            else {
                break;
            };
            // SAFETY: fetch_preset returned a fresh name.
            let name = unsafe { owned(name) }.expect("a preset has a name");
            presets.push((name, params));
        }
        presets
    }

    /// `new_desc` for valid parameters, seeded with `seed`: the
    /// description and aux string.
    ///
    /// # Panics
    /// If `params` contains a NUL byte or the seed is huge.
    #[must_use]
    pub fn new_desc(
        self,
        params: &str,
        seed: &[u8],
        interactive: bool,
    ) -> (String, Option<String>) {
        let params = c_string(params);
        let len = c_int::try_from(seed.len()).expect("seed fits in an int");
        let mut aux = ptr::null_mut();
        // SAFETY: valid game, string and seed buffer; returns fresh strings.
        let desc = unsafe {
            owned(oracle_new_desc(
                self.ptr(),
                params.as_ptr(),
                seed.as_ptr().cast(),
                len,
                interactive,
                &raw mut aux,
            ))
        }
        .expect("new_desc returns a string");
        // SAFETY: aux is NULL or fresh.
        (desc, unsafe { owned(aux) })
    }

    /// `validate_desc` for valid parameters.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If a string contains a NUL byte.
    pub fn validate_desc(self, params: &str, desc: &str) -> Result<(), String> {
        let (params, desc) = (c_string(params), c_string(desc));
        // SAFETY: valid game and strings; returns NULL or a static string.
        let err = unsafe {
            borrowed(oracle_validate_desc(
                self.ptr(),
                params.as_ptr(),
                desc.as_ptr(),
            ))
        };
        err.map_or(Ok(()), Err)
    }
}

/// What C's `interpret_move` returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CMoveResult {
    /// `MOVE_UNUSED`.
    Unused,
    /// `MOVE_NO_EFFECT`.
    NoEffect,
    /// `MOVE_UI_UPDATE`.
    UiUpdate,
    /// A move string.
    Move(String),
}

/// A C game in progress (see `csrc/game_harness.c`): original, previous
/// and current states, a UI, and a draw state drawing into a recorder.
pub struct CSession {
    s: NonNull<SessionC>,
}

impl Drop for CSession {
    fn drop(&mut self) {
        // SAFETY: created by oracle_session_new, freed once.
        unsafe { oracle_session_free(self.s.as_ptr()) }
    }
}

impl CSession {
    /// A session on a description that passed `validate_desc`, sized for
    /// `tilesize`.
    ///
    /// # Panics
    /// If a string contains a NUL byte.
    #[must_use]
    pub fn new(game: CGame, params: &str, desc: &str, tilesize: i32) -> Self {
        let (params, desc) = (c_string(params), c_string(desc));
        let _globals = super::lock_c_globals();
        // SAFETY: valid game and strings; returns a fresh session.
        let s = unsafe { oracle_session_new(game.ptr(), params.as_ptr(), desc.as_ptr(), tilesize) };
        Self {
            s: NonNull::new(s).expect("session allocation failed"),
        }
    }

    fn ptr(&self) -> *mut SessionC {
        self.s.as_ptr()
    }

    /// `interpret_move` on the current state, with a raw button code.
    ///
    /// # Panics
    /// If C returns a NULL move string.
    pub fn interpret(&mut self, x: i32, y: i32, button: i32) -> CMoveResult {
        let mut mv = ptr::null_mut();
        // SAFETY: live session; a move comes back fresh.
        let kind = unsafe { oracle_session_interpret(self.ptr(), x, y, button, &raw mut mv) };
        match kind {
            0 => CMoveResult::Unused,
            1 => CMoveResult::NoEffect,
            2 => CMoveResult::UiUpdate,
            // SAFETY: a fresh move string.
            _ => CMoveResult::Move(unsafe { owned(mv) }.expect("a move string")),
        }
    }

    /// `execute_move` on the current state; on success it becomes the
    /// previous state (after `changed_state`). False if C rejected it.
    ///
    /// # Panics
    /// If `mv` contains a NUL byte.
    pub fn execute(&mut self, mv: &str) -> bool {
        let mv = c_string(mv);
        // SAFETY: live session and valid string.
        unsafe { oracle_session_execute(self.ptr(), mv.as_ptr()) }
    }

    /// `anim_length` from the previous state to the current one.
    pub fn anim_length(&mut self, dir: i32) -> f32 {
        // SAFETY: live session.
        unsafe { oracle_session_anim_length(self.ptr(), dir) }
    }

    /// `flash_length` from the previous state to the current one.
    pub fn flash_length(&mut self, dir: i32) -> f32 {
        // SAFETY: live session.
        unsafe { oracle_session_flash_length(self.ptr(), dir) }
    }

    /// `redraw` of the current state (from the previous one if
    /// `animate`): this redraw's trace, and the last status bar text.
    ///
    /// # Panics
    /// If C returns no trace.
    pub fn redraw(
        &mut self,
        animate: bool,
        dir: i32,
        anim_time: f32,
        flash_time: f32,
    ) -> (String, Option<String>) {
        let mut status = ptr::null();
        // SAFETY: live session; both strings are owned by it and copied
        // before the next call.
        unsafe {
            let trace = oracle_session_redraw(
                self.ptr(),
                animate,
                dir,
                anim_time,
                flash_time,
                &raw mut status,
            );
            (borrowed(trace).expect("a trace"), borrowed(status))
        }
    }

    /// `solve(orig, cur, aux)`.
    ///
    /// # Errors
    /// C's message.
    ///
    /// # Panics
    /// If `aux` contains a NUL byte.
    pub fn solve(&mut self, aux: Option<&str>) -> Result<String, String> {
        let aux = aux.map(c_string);
        let aux_ptr = aux.as_ref().map_or(ptr::null(), |a| a.as_ptr());
        let mut error = ptr::null();
        // SAFETY: live session; a fresh move or a static error comes back.
        unsafe {
            owned(oracle_session_solve(self.ptr(), aux_ptr, &raw mut error))
                .ok_or_else(|| borrowed(error).unwrap_or_default())
        }
    }

    /// `text_format` of the current state, if the game has one.
    pub fn text_format(&mut self) -> Option<String> {
        // SAFETY: live session; NULL or a fresh string.
        unsafe { owned(oracle_session_text_format(self.ptr())) }
    }

    /// `encode_ui` of the UI, if the game has one.
    pub fn encode_ui(&mut self) -> Option<String> {
        // SAFETY: live session; NULL or a fresh string.
        unsafe { owned(oracle_session_encode_ui(self.ptr())) }
    }

    /// `status` of the current state: 0, +1 or -1.
    pub fn status(&mut self) -> i32 {
        // SAFETY: live session.
        unsafe { oracle_session_status(self.ptr()) }
    }

    /// `current_key_label` ("" for none).
    pub fn key_label(&mut self, button: i32) -> String {
        // SAFETY: live session; a static string.
        unsafe { borrowed(oracle_session_key_label(self.ptr(), button)) }.unwrap_or_default()
    }

    /// `get_cursor_location`: `(x, y, w, h)`, or `None` if hidden.
    pub fn cursor(&mut self) -> Option<(i32, i32, i32, i32)> {
        let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
        // SAFETY: live session and valid out-pointers.
        let shown = unsafe {
            oracle_session_cursor(self.ptr(), &raw mut x, &raw mut y, &raw mut w, &raw mut h)
        };
        shown.then_some((x, y, w, h))
    }

    /// `compute_size` for the session's parameters.
    pub fn size(&mut self, tilesize: i32) -> (i32, i32) {
        let (mut x, mut y) = (0, 0);
        // SAFETY: live session and valid out-pointers.
        unsafe { oracle_session_size(self.ptr(), tilesize, &raw mut x, &raw mut y) };
        (x, y)
    }
}

/// C's `game_mkhighlight_specific` on one base colour: the base (moved if
/// it was too close to white or black), the highlight and the lowlight.
#[must_use]
pub fn c_mkhighlight(base: [f32; 3]) -> [[f32; 3]; 3] {
    let mut out = [0.0_f32; 9];
    // SAFETY: `base` holds 3 floats, `out` 9.
    unsafe { oracle_mkhighlight(base.as_ptr(), out.as_mut_ptr()) };
    [
        [out[0], out[1], out[2]],
        [out[3], out[4], out[5]],
        [out[6], out[7], out[8]],
    ]
}

/*
 * game_harness.c: drives any C game (a `const game *`) the way the
 * mid-end does, one back-end call at a time, so Rust tests can make the
 * same calls on the port and compare. Parameters, descriptions and moves
 * go in and out as text; drawing goes to a recording drawing
 * (recorder.c).
 *
 * Strings returned to Rust are allocated with smalloc (free with sfree)
 * unless a comment says the harness owns them.
 */

#include <limits.h>
#include <stdbool.h>
#include <string.h>

#include "puzzles.h"

drawing *oracle_game_recorder_new(void);
void oracle_recorder_clear(drawing *dr);
const char *oracle_recorder_text(drawing *dr);
const char *oracle_recorder_status(drawing *dr);
void oracle_recorder_free(drawing *dr);

/* ---------------------------------------------------------------------
 * Parameters and generation.
 */

static game_params *decode(const game *g, const char *params)
{
    game_params *p = g->default_params();
    g->decode_params(p, params);
    return p;
}

/* encode_params(decode_params(default_params(), params), full). */
char *oracle_encode_params(const game *g, const char *params, bool full)
{
    game_params *p = decode(g, params);
    char *ret = g->encode_params(p, full);
    g->free_params(p);
    return ret;
}

/* validate_params's error, or NULL (a static string). */
const char *oracle_validate_params(const game *g, const char *params,
                                   bool full)
{
    game_params *p = decode(g, params);
    const char *err = g->validate_params(p, full);
    g->free_params(p);
    return err;
}

/* The presets, by fetch_preset: the full encoding of preset i and its
 * name in *name, or NULL past the end. */
char *oracle_preset(const game *g, int i, char **name)
{
    game_params *p;
    char *ret;

    if (!g->fetch_preset || !g->fetch_preset(i, name, &p))
        return NULL;
    ret = g->encode_params(p, true);
    g->free_params(p);
    return ret;
}

/* new_desc for valid parameters, from a generator seeded with `seed`;
 * the aux string (or NULL) goes to *aux. */
char *oracle_new_desc(const game *g, const char *params, const char *seed,
                      int seedlen, bool interactive, char **aux)
{
    game_params *p = decode(g, params);
    random_state *rs = random_new(seed, seedlen);
    char *desc;

    *aux = NULL;
    desc = g->new_desc(p, rs, aux, interactive);
    random_free(rs);
    g->free_params(p);
    return desc;
}

/* validate_desc's error, or NULL (a static string). */
const char *oracle_validate_desc(const game *g, const char *params,
                                 const char *desc)
{
    game_params *p = decode(g, params);
    const char *err = g->validate_desc(p, desc);
    g->free_params(p);
    return err;
}

/* ---------------------------------------------------------------------
 * A game in progress: the original and current states, the state
 * before the last move, a UI and a draw state with a recording drawing.
 */

typedef struct oracle_session {
    const game *g;
    game_params *params;
    game_state *orig, *prev, *cur;
    game_ui *ui;
    game_drawstate *ds;
    drawing *dr;
} oracle_session;

/* A session on a valid description, sized for `tilesize`. */
oracle_session *oracle_session_new(const game *g, const char *params,
                                   const char *desc, int tilesize)
{
    oracle_session *s = snew(oracle_session);

    s->g = g;
    s->params = decode(g, params);
    s->orig = g->new_game(NULL, s->params, desc);
    s->cur = g->dup_game(s->orig);
    s->prev = NULL;
    s->ui = g->new_ui(s->cur);
    s->dr = oracle_game_recorder_new();
    s->ds = g->new_drawstate(s->dr, s->cur);
    g->set_size(s->dr, s->ds, s->params, tilesize);
    return s;
}

void oracle_session_free(oracle_session *s)
{
    const game *g = s->g;

    g->free_drawstate(s->dr, s->ds);
    oracle_recorder_free(s->dr);
    g->free_ui(s->ui);
    if (s->prev)
        g->free_game(s->prev);
    g->free_game(s->cur);
    g->free_game(s->orig);
    g->free_params(s->params);
    sfree(s);
}

/*
 * interpret_move: 0 MOVE_UNUSED (or NULL, which the mid-end treats the
 * same), 1 MOVE_NO_EFFECT, 2 MOVE_UI_UPDATE, or 3 with the move string
 * in *move.
 */
int oracle_session_interpret(oracle_session *s, int x, int y, int button,
                             char **move)
{
    char *ret = s->g->interpret_move(s->cur, s->ui, s->ds, x, y, button);

    *move = NULL;
    if (ret == NULL || ret == MOVE_UNUSED)
        return 0;
    if (ret == MOVE_NO_EFFECT)
        return 1;
    if (ret == MOVE_UI_UPDATE)
        return 2;
    *move = ret;
    return 3;
}

/*
 * execute_move on the current state. On success, the old current state
 * becomes the previous one, changed_state runs as in the mid-end, and
 * the result is true.
 */
bool oracle_session_execute(oracle_session *s, const char *move)
{
    game_state *next = s->g->execute_move(s->cur, move);

    if (!next)
        return false;
    s->g->changed_state(s->ui, s->cur, next);
    if (s->prev)
        s->g->free_game(s->prev);
    s->prev = s->cur;
    s->cur = next;
    return true;
}

/* anim_length and flash_length from the previous state to the current
 * one; 0 if there is no previous state. */
float oracle_session_anim_length(oracle_session *s, int dir)
{
    return s->prev ? s->g->anim_length(s->prev, s->cur, dir, s->ui) : 0.0F;
}

float oracle_session_flash_length(oracle_session *s, int dir)
{
    return s->prev ? s->g->flash_length(s->prev, s->cur, dir, s->ui) : 0.0F;
}

/*
 * redraw of the current state, animating from the previous one if
 * `animate`. Returns the trace of this redraw (owned by the session,
 * valid until the next redraw) and the last status text in *status
 * (owned by the session; NULL if none yet).
 */
const char *oracle_session_redraw(oracle_session *s, bool animate, int dir,
                                  float anim_time, float flash_time,
                                  const char **status)
{
    oracle_recorder_clear(s->dr);
    s->g->redraw(s->dr, s->ds, animate ? s->prev : NULL, s->cur, dir,
                 s->ui, anim_time, flash_time);
    *status = oracle_recorder_status(s->dr);
    return oracle_recorder_text(s->dr);
}

/* solve(orig, cur, aux): the move string, or NULL with the error in
 * *error (a static string). */
char *oracle_session_solve(oracle_session *s, const char *aux,
                           const char **error)
{
    *error = NULL;
    return s->g->solve(s->orig, s->cur, aux, error);
}

/* text_format of the current state, or NULL if the game has none. */
char *oracle_session_text_format(oracle_session *s)
{
    if (!s->g->can_format_as_text_ever ||
        !s->g->can_format_as_text_now(s->params))
        return NULL;
    return s->g->text_format(s->cur);
}

int oracle_session_status(oracle_session *s)
{
    return s->g->status(s->cur);
}

/* current_key_label, "" if the game has none (a static string). */
const char *oracle_session_key_label(oracle_session *s, int button)
{
    if (!s->g->current_key_label)
        return "";
    return s->g->current_key_label(s->ui, s->cur, button);
}

/* get_cursor_location: false if the game left it unset (hidden). */
bool oracle_session_cursor(oracle_session *s, int *x, int *y, int *w, int *h)
{
    *x = *y = *w = *h = INT_MIN;
    if (s->g->get_cursor_location)
        s->g->get_cursor_location(s->ui, s->ds, s->cur, s->params,
                                  x, y, w, h);
    return *x != INT_MIN;
}

/* compute_size for the session's parameters and UI. */
void oracle_session_size(oracle_session *s, int tilesize, int *x, int *y)
{
    s->g->compute_size(s->params, tilesize, s->ui, x, y);
}

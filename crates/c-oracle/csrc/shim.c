/*
 * shim.c: platform functions the upstream C code expects a front end to
 * provide, implemented for running it as a test oracle.
 *
 * Only what the compiled sources actually reference lives here; stubs are
 * added as more of the C code is compiled into the oracle.
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>

#include "puzzles.h"

/* Called by smalloc/srealloc on allocation failure: report and abort. */
void fatal(const char *fmt, ...)
{
    va_list ap;

    fputs("c-oracle fatal error: ", stderr);
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
    abort();
}

/*
 * Link-time placeholders for functions referenced by compiled sources but
 * not exercised by any oracle test yet. Each aborts loudly if called.
 * Delete each one when the real implementation is compiled into the
 * oracle: frontend_default_colour (misc.c's game_mkhighlight and the
 * games' colours()) with the theme work, document_add_puzzle with
 * printing. The other front-end functions are in midend_harness.c.
 */
static void not_in_oracle(const char *name)
{
    fatal("%s is not available in the oracle yet", name);
}

void frontend_default_colour(frontend *fe, float *output)
{
    not_in_oracle("frontend_default_colour");
}

/* Printing (midend_print_puzzle) is not compiled in yet. */
void document_add_puzzle(document *doc, const game *game, game_params *par,
                         game_ui *ui, game_state *st, game_state *st2)
{
    not_in_oracle("document_add_puzzle");
}

/*
 * The button classification macros of puzzles.h, as functions Rust can
 * call: bit 0 IS_MOUSE_DOWN, 1 IS_MOUSE_DRAG, 2 IS_MOUSE_RELEASE,
 * 3 IS_CURSOR_MOVE, 4 IS_CURSOR_SELECT, 5 IS_UI_FAKE_KEY.
 */
int oracle_button_class(int button)
{
    return (IS_MOUSE_DOWN(button) ? 1 : 0) |
        (IS_MOUSE_DRAG(button) ? 2 : 0) |
        (IS_MOUSE_RELEASE(button) ? 4 : 0) |
        (IS_CURSOR_MOVE(button) ? 8 : 0) |
        (IS_CURSOR_SELECT(button) ? 16 : 0) |
        (IS_UI_FAKE_KEY(button) ? 32 : 0);
}

/*
 * move_cursor's result as a number: 0 MOVE_UNUSED, 1 MOVE_NO_EFFECT,
 * 2 MOVE_UI_UPDATE (it never returns a real move).
 */
int oracle_move_cursor(int button, int *x, int *y, int maxw, int maxh,
                       bool wrap, bool *visible)
{
    char *ret = move_cursor(button, x, y, maxw, maxh, wrap, visible);
    if (ret == MOVE_UNUSED)
        return 0;
    if (ret == MOVE_NO_EFFECT)
        return 1;
    if (ret == MOVE_UI_UPDATE)
        return 2;
    fatal("move_cursor returned an unexpected pointer");
    return -1;
}

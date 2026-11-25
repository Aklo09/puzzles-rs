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
 * Link-time placeholders for functions referenced by compiled sources
 * (misc.c's drawing and colour helpers) but not exercised by any oracle
 * test yet. Each aborts loudly if called. Delete each one when the real
 * implementation (drawing.c, a recording drawing API, a front-end colour)
 * is compiled into the oracle in Phase 2.
 */
static void not_in_oracle(const char *name)
{
    fatal("%s is not available in the oracle yet", name);
}

void draw_text(drawing *dr, int x, int y, int fonttype, int fontsize,
               int align, int colour, const char *text)
{
    not_in_oracle("draw_text");
}

void draw_line(drawing *dr, int x1, int y1, int x2, int y2, int colour)
{
    not_in_oracle("draw_line");
}

void draw_polygon(drawing *dr, const int *coords, int npoints,
                  int fillcolour, int outlinecolour)
{
    not_in_oracle("draw_polygon");
}

void frontend_default_colour(frontend *fe, float *output)
{
    not_in_oracle("frontend_default_colour");
}

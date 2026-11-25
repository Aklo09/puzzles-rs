/*
 * recorder.c: a drawing_api that records every call as a line of text,
 * for draw-trace tests. The format matches puzzles_core's
 * RecordingDrawing (drawing.rs) exactly: integers in decimal, a missing
 * fill as -1, floats as the hex of their bits, text last and verbatim.
 *
 * Like C front ends without one, it provides no text_fallback (so
 * drawing.c's ASCII fallback runs) and no status_bar (the status text is
 * the mid-end's business, compared separately).
 */

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "puzzles.h"

struct blitter {
    int w, h;
};

typedef struct recorder {
    char *text;
    size_t len, size;
} recorder;

static void rec_printf(drawing *dr, const char *fmt, ...)
{
    recorder *rec = GET_HANDLE_AS_TYPE(dr, recorder);
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (rec->len + n + 1 > rec->size) {
        rec->size = (rec->len + n + 1) * 2;
        rec->text = sresize(rec->text, rec->size, char);
    }
    va_start(ap, fmt);
    vsnprintf(rec->text + rec->len, n + 1, fmt, ap);
    va_end(ap);
    rec->len += n;
}

static uint32_t bits(float f)
{
    uint32_t u;
    memcpy(&u, &f, sizeof(u));
    return u;
}

static void rec_draw_text(drawing *dr, int x, int y, int fonttype,
                          int fontsize, int align, int colour,
                          const char *text)
{
    rec_printf(dr, "text %d %d %d %d %d %d %s\n", x, y, fonttype, fontsize,
               align, colour, text);
}

static void rec_draw_rect(drawing *dr, int x, int y, int w, int h, int colour)
{
    rec_printf(dr, "rect %d %d %d %d %d\n", x, y, w, h, colour);
}

static void rec_draw_line(drawing *dr, int x1, int y1, int x2, int y2,
                          int colour)
{
    rec_printf(dr, "line %d %d %d %d %d\n", x1, y1, x2, y2, colour);
}

static void rec_draw_polygon(drawing *dr, const int *coords, int npoints,
                             int fillcolour, int outlinecolour)
{
    int i;
    rec_printf(dr, "polygon %d %d %d", fillcolour, outlinecolour, npoints);
    for (i = 0; i < npoints; i++)
        rec_printf(dr, " %d %d", coords[2*i], coords[2*i+1]);
    rec_printf(dr, "\n");
}

static void rec_draw_circle(drawing *dr, int cx, int cy, int radius,
                            int fillcolour, int outlinecolour)
{
    rec_printf(dr, "circle %d %d %d %d %d\n", cx, cy, radius, fillcolour,
               outlinecolour);
}

static void rec_draw_thick_line(drawing *dr, float thickness, float x1,
                                float y1, float x2, float y2, int colour)
{
    rec_printf(dr, "thick %08lx %08lx %08lx %08lx %08lx %d\n",
               (unsigned long)bits(thickness), (unsigned long)bits(x1),
               (unsigned long)bits(y1), (unsigned long)bits(x2),
               (unsigned long)bits(y2), colour);
}

static void rec_draw_update(drawing *dr, int x, int y, int w, int h)
{
    rec_printf(dr, "update %d %d %d %d\n", x, y, w, h);
}

static void rec_clip(drawing *dr, int x, int y, int w, int h)
{
    rec_printf(dr, "clip %d %d %d %d\n", x, y, w, h);
}

static void rec_unclip(drawing *dr)
{
    rec_printf(dr, "unclip\n");
}

static void rec_start_draw(drawing *dr)
{
    rec_printf(dr, "start\n");
}

static void rec_end_draw(drawing *dr)
{
    rec_printf(dr, "end\n");
}

static blitter *rec_blitter_new(drawing *dr, int w, int h)
{
    blitter *bl = snew(blitter);
    bl->w = w;
    bl->h = h;
    return bl;
}

static void rec_blitter_free(drawing *dr, blitter *bl)
{
    sfree(bl);
}

static void rec_blitter_save(drawing *dr, blitter *bl, int x, int y)
{
    rec_printf(dr, "blitter_save %d %d %d %d\n", bl->w, bl->h, x, y);
}

static void rec_blitter_load(drawing *dr, blitter *bl, int x, int y)
{
    rec_printf(dr, "blitter_load %d %d %d %d\n", bl->w, bl->h, x, y);
}

static const drawing_api recorder_api = {
    1,
    rec_draw_text,
    rec_draw_rect,
    rec_draw_line,
    rec_draw_polygon,
    rec_draw_circle,
    rec_draw_update,
    rec_clip,
    rec_unclip,
    rec_start_draw,
    rec_end_draw,
    NULL, /* status_bar */
    rec_blitter_new,
    rec_blitter_free,
    rec_blitter_save,
    rec_blitter_load,
    NULL, NULL, NULL, NULL, NULL, NULL, /* printing */
    NULL, NULL, /* line_width, line_dotted */
    NULL, /* text_fallback */
    rec_draw_thick_line,
};

/* A drawing that records into a fresh buffer. */
drawing *oracle_recorder_new(void)
{
    recorder *rec = snew(recorder);
    rec->size = 256;
    rec->len = 0;
    rec->text = snewn(rec->size, char);
    rec->text[0] = '\0';
    return drawing_new(&recorder_api, NULL, rec);
}

/* The text recorded so far (owned by the recorder). */
const char *oracle_recorder_text(drawing *dr)
{
    return GET_HANDLE_AS_TYPE(dr, recorder)->text;
}

void oracle_recorder_free(drawing *dr)
{
    recorder *rec = GET_HANDLE_AS_TYPE(dr, recorder);
    sfree(rec->text);
    sfree(rec);
    drawing_free(dr);
}

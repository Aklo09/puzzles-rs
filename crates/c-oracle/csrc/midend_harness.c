/*
 * midend_harness.c: a minimal C front end for driving midend.c from
 * Rust tests: the front-end functions the mid-end calls
 * (activate_timer, deactivate_timer, get_random_seed), and flat
 * wrappers taking and returning text.
 *
 * Like a real front end, a test redraws after each event it sends
 * (oracle_midend_redraw); the drawing records into a game recorder
 * (recorder.c), which keeps the last status bar text.
 *
 * Strings returned to Rust are allocated with smalloc (free with sfree)
 * unless a comment says otherwise.
 */

#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "puzzles.h"

const drawing_api *oracle_game_recorder_api(void);
void *oracle_recorder_handle_new(void);
const char *oracle_recorder_handle_text(void *handle);
const char *oracle_recorder_handle_status(void *handle);
void oracle_recorder_handle_clear(void *handle);
void oracle_recorder_handle_free(void *handle);

/* ---------------------------------------------------------------------
 * Front-end functions.
 */

struct frontend {
    bool timer;
};

void activate_timer(frontend *fe)
{
    if (fe)
        fe->timer = true;
}

void deactivate_timer(frontend *fe)
{
    if (fe)
        fe->timer = false;
}

/* The seed the next midend_new gets (set by oracle_midend_new). */
static const char *next_seed = "";
static int next_seed_len = 0;

void get_random_seed(void **randseed, int *randseedsize)
{
    char *seed = snewn(next_seed_len + 1, char);
    memcpy(seed, next_seed, next_seed_len);
    *randseed = seed;
    *randseedsize = next_seed_len;
}

/* ---------------------------------------------------------------------
 * Buffers for serialisation.
 */

typedef struct buffer {
    char *data;
    int len, size;
} buffer;

static void buffer_write(void *ctx, const void *buf, int len)
{
    buffer *b = (buffer *)ctx;
    if (b->len + len + 1 > b->size) {
        b->size = (b->len + len + 1) * 2;
        b->data = sresize(b->data, b->size, char);
    }
    memcpy(b->data + b->len, buf, len);
    b->len += len;
    b->data[b->len] = '\0';
}

typedef struct reader {
    const char *data;
    int len, pos;
} reader;

static bool buffer_read(void *ctx, void *buf, int len)
{
    reader *r = (reader *)ctx;
    if (len > r->len - r->pos)
        return false;
    memcpy(buf, r->data + r->pos, len);
    r->pos += len;
    return true;
}

/* ---------------------------------------------------------------------
 * The mid-end.
 */

typedef struct oracle_midend {
    midend *me;
    const game *g;
    frontend fe;
    void *rec;                         /* recorder handle, or NULL */
    int npresets;                      /* -1 until the menu is built */
} oracle_midend;

/* A mid-end whose random seed generator starts from `seed`;
 * non-interactive (no drawing) unless `interactive`. */
oracle_midend *oracle_midend_new(const game *g, const char *seed, int seedlen,
                                 bool interactive)
{
    oracle_midend *om = snew(oracle_midend);

    om->g = g;
    om->fe.timer = false;
    om->rec = interactive ? oracle_recorder_handle_new() : NULL;
    om->npresets = -1;
    next_seed = seed;
    next_seed_len = seedlen;
    om->me = midend_new(&om->fe, g, interactive ? oracle_game_recorder_api() : NULL,
                        om->rec);
    next_seed = "";
    next_seed_len = 0;
    return om;
}

void oracle_midend_free(oracle_midend *om)
{
    midend_free(om->me);
    if (om->rec)
        oracle_recorder_handle_free(om->rec);
    sfree(om);
}

void oracle_midend_size(oracle_midend *om, int *x, int *y, bool user_size,
                        double dpr)
{
    midend_size(om->me, x, y, user_size, dpr);
}

int oracle_midend_tilesize(oracle_midend *om)
{
    return midend_tilesize(om->me);
}

void oracle_midend_new_game(oracle_midend *om)
{
    midend_new_game(om->me);
}

void oracle_midend_restart_game(oracle_midend *om)
{
    midend_restart_game(om->me);
}

int oracle_midend_process_key(oracle_midend *om, int x, int y, int button)
{
    return midend_process_key(om->me, x, y, button);
}

void oracle_midend_timer(oracle_midend *om, float tplus)
{
    midend_timer(om->me, tplus);
}

void oracle_midend_stop_anim(oracle_midend *om)
{
    midend_stop_anim(om->me);
}

bool oracle_midend_wants_timer(oracle_midend *om)
{
    return om->fe.timer;
}

/* A redraw, as a front end does after each event (interactive only). */
void oracle_midend_redraw(oracle_midend *om)
{
    if (om->rec)
        midend_redraw(om->me);
}

/* midend_force_redraw: returns its trace (owned by the harness). */
const char *oracle_midend_force_redraw(oracle_midend *om)
{
    oracle_recorder_handle_clear(om->rec);
    midend_force_redraw(om->me);
    return oracle_recorder_handle_text(om->rec);
}

/* The last status bar text the front end got, or NULL (owned by the
 * harness). */
const char *oracle_midend_status_text(oracle_midend *om)
{
    return om->rec ? oracle_recorder_handle_status(om->rec) : NULL;
}

/* midend_solve's error, or NULL (a static string). */
const char *oracle_midend_solve(oracle_midend *om)
{
    return midend_solve(om->me);
}

int oracle_midend_status(oracle_midend *om)
{
    return midend_status(om->me);
}

bool oracle_midend_can_undo(oracle_midend *om)
{
    return midend_can_undo(om->me);
}

bool oracle_midend_can_redo(oracle_midend *om)
{
    return midend_can_redo(om->me);
}

char *oracle_midend_text_format(oracle_midend *om)
{
    return midend_text_format(om->me);
}

/* midend_game_id's error, or NULL (a static string). */
const char *oracle_midend_game_id(oracle_midend *om, const char *id)
{
    return midend_game_id(om->me, id);
}

char *oracle_midend_get_game_id(oracle_midend *om)
{
    return midend_get_game_id(om->me);
}

char *oracle_midend_get_random_seed(oracle_midend *om)
{
    return midend_get_random_seed(om->me);
}

/* midend_set_params with parameters decoded over the defaults. */
void oracle_midend_set_params(oracle_midend *om, const char *params)
{
    game_params *p = om->g->default_params();
    om->g->decode_params(p, params);
    midend_set_params(om->me, p);
    om->g->free_params(p);
}

/* Builds the preset menu; returns the number of ids. (Once built,
 * midend_get_presets returns early without setting the count.) */
int oracle_midend_presets(oracle_midend *om)
{
    if (om->npresets < 0)
        midend_get_presets(om->me, &om->npresets);
    return om->npresets;
}

/* Chooses preset `id` (from oracle_midend_presets); false if it isn't
 * a preset. */
bool oracle_midend_set_preset(oracle_midend *om, int id)
{
    game_params *p = preset_menu_lookup_by_id(
        midend_get_presets(om->me, NULL), id);
    if (!p)
        return false;
    midend_set_params(om->me, p);
    return true;
}

int oracle_midend_which_preset(oracle_midend *om)
{
    return midend_which_preset(om->me);
}

/* A config dialog as text: the title, then a line per item:
 * name|keyword|type|value, the value being the string, the selected
 * index and the choice names joined by ',' (then '/' and the keywords),
 * or 0/1. NULL if there is no dialog. */
char *oracle_midend_get_config(oracle_midend *om, int which)
{
    char *title;
    config_item *cfg = midend_get_config(om->me, which, &title);
    buffer b = { NULL, 0, 0 };
    int i;

    if (!cfg)
        return NULL;
    buffer_write(&b, title, strlen(title));
    buffer_write(&b, "\n", 1);
    sfree(title);
    for (i = 0; cfg[i].type != C_END; i++) {
        char num[32];
        const char *kw = cfg[i].kw ? cfg[i].kw : "-";
        buffer_write(&b, cfg[i].name, strlen(cfg[i].name));
        buffer_write(&b, "|", 1);
        buffer_write(&b, kw, strlen(kw));
        buffer_write(&b, "|", 1);
        switch (cfg[i].type) {
          case C_STRING:
            buffer_write(&b, "string|", 7);
            buffer_write(&b, cfg[i].u.string.sval,
                         strlen(cfg[i].u.string.sval));
            break;
          case C_CHOICES: {
            const char *names = cfg[i].u.choices.choicenames;
            const char *kws = cfg[i].u.choices.choicekws;
            int j;
            sprintf(num, "choices|%d|", cfg[i].u.choices.selected);
            buffer_write(&b, num, strlen(num));
            for (j = 1; names[j]; j++)
                buffer_write(&b, names[j] == names[0] ? "," : names + j, 1);
            if (kws) {
                buffer_write(&b, "/", 1);
                for (j = 1; kws[j]; j++)
                    buffer_write(&b, kws[j] == kws[0] ? "," : kws + j, 1);
            }
            break;
          }
          case C_BOOLEAN:
            buffer_write(&b, cfg[i].u.boolean.bval ? "bool|1" : "bool|0", 6);
            break;
        }
        buffer_write(&b, "\n", 1);
    }
    free_cfg(cfg);
    return b.data;
}

/* midend_set_config with dialog `which` as midend_get_config fills it,
 * the n values replacing the items' in order: a string, a choice index
 * in decimal, or "0"/"1". Returns the error, or NULL (a static
 * string). */
const char *oracle_midend_set_config(oracle_midend *om, int which,
                                     const char *const *values, int n)
{
    char *title;
    config_item *cfg = midend_get_config(om->me, which, &title);
    const char *err;
    int i;

    if (!cfg)
        return "no such dialog";
    sfree(title);
    for (i = 0; cfg[i].type != C_END && i < n; i++) {
        switch (cfg[i].type) {
          case C_STRING:
            sfree(cfg[i].u.string.sval);
            cfg[i].u.string.sval = dupstr(values[i]);
            break;
          case C_CHOICES:
            cfg[i].u.choices.selected = atoi(values[i]);
            break;
          case C_BOOLEAN:
            cfg[i].u.boolean.bval = values[i][0] == '1';
            break;
        }
    }
    err = midend_set_config(om->me, which, cfg);
    free_cfg(cfg);
    return err;
}

/* midend_serialise; the length goes to *len. */
char *oracle_midend_serialise(oracle_midend *om, int *len)
{
    buffer b = { NULL, 0, 0 };
    buffer_write(&b, "", 0);
    midend_serialise(om->me, buffer_write, &b);
    *len = b.len;
    return b.data;
}

/* midend_deserialise's error, or NULL (a static string). */
const char *oracle_midend_deserialise(oracle_midend *om, const char *data,
                                      int len)
{
    reader r = { data, len, 0 };
    return midend_deserialise(om->me, buffer_read, &r);
}

/* identify_game's error, or NULL with the name in *name. */
const char *oracle_identify_game(const char *data, int len, char **name)
{
    reader r = { data, len, 0 };
    return identify_game(name, buffer_read, &r);
}

char *oracle_midend_save_prefs(oracle_midend *om, int *len)
{
    buffer b = { NULL, 0, 0 };
    buffer_write(&b, "", 0);
    midend_save_prefs(om->me, buffer_write, &b);
    *len = b.len;
    return b.data;
}

/* midend_load_prefs's error, or NULL (a static string). */
const char *oracle_midend_load_prefs(oracle_midend *om, const char *data,
                                     int len)
{
    reader r = { data, len, 0 };
    return midend_load_prefs(om->me, buffer_read, &r);
}

/* midend_get_cursor_location: false if there is none. */
bool oracle_midend_cursor(oracle_midend *om, int *x, int *y, int *w, int *h)
{
    return midend_get_cursor_location(om->me, x, y, w, h);
}

/* printf("%g") of a float promoted to double, as the TIME line uses. */
void oracle_format_g(double x, char *out, int size)
{
    snprintf(out, size, "%g", x);
}

/* atof, as the TIME line is read. */
double oracle_atof(const char *s)
{
    return atof(s);
}

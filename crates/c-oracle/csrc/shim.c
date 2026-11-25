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

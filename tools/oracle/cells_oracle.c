/*
 * Observes the pinned cbirds cells.c: runs a script of calls read from stdin
 * against one cells_t and prints what each did, for tests/cells_oracle.rs to
 * compare with the Rust translation. It only calls the reference's functions
 * and prints their results.
 *
 * Script: one command a line, fields separated by spaces.
 *   I t                  cells_destroy (if initialised) then cells_init(t)
 *   T t                  cells.truecolor = t
 *   R cols rows          cells_resize
 *   K cols rows          cells_keep_out_of
 *   V                    cells_invalidate
 *   C w h\n<w*h*4 bytes> the canvas: w*h RGBA pixels, or none (NULL) if w is 0
 *   D style cw ch        cells_read (style 0 braille, 1 sextants, 2 blocks)
 *   E                    cells_emit
 *   P style cw ch r g b  cells_paint into a fresh image, on ground r g b
 *   S                    the whole cells_t
 *   G grid i glyph fr fg fb br bg bb hf hb
 *                        sets cell i of now (grid 0) or before (grid 1)
 *
 * Transcript (stdout): see each command below; the Rust side prints the same.
 */
#include "cells.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static cells_t cells;
static int initialised;
static png_image_t canvas = {0, 0, NULL};

static cells_style_t style_of(int code) {
    return code == 0 ? CELLS_BRAILLE : code == 1 ? CELLS_SEXTANTS : CELLS_BLOCKS;
}

static void print_escaped(const char *bytes, size_t length) {
    for (size_t i = 0; i < length; i++) {
        unsigned char c = (unsigned char)bytes[i];
        if (c >= 0x20 && c < 0x7f && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

static void print_grid(const char *name, const cell_t *grid) {
    if (grid == NULL) {
        printf("%s null\n", name);
        return;
    }
    for (int row = 0; row < cells.rows; row++) {
        printf("%s %d:", name, row);
        for (int col = 0; col < cells.cols; col++) {
            const cell_t *c = &grid[(size_t)row * (size_t)cells.cols + (size_t)col];
            printf(" %x,%02x%02x%02x,%02x%02x%02x,%d%d", c->glyph, c->fg[0], c->fg[1], c->fg[2],
                   c->bg[0], c->bg[1], c->bg[2], c->has_fg, c->has_bg);
        }
        putchar('\n');
    }
}

static void fail(const char *why) {
    fprintf(stderr, "cells_oracle: %s\n", why);
    exit(2);
}

int main(void) {
    char command;
    while (scanf(" %c", &command) == 1) {
        switch (command) {
            case 'I': {
                int truecolor;
                if (scanf("%d", &truecolor) != 1) fail("bad I");
                if (initialised) cells_destroy(&cells);
                printf("init %s\n", cells_status_string(cells_init(&cells, truecolor)));
                initialised = 1;
                break;
            }
            case 'T': {
                int truecolor;
                if (scanf("%d", &truecolor) != 1) fail("bad T");
                cells.truecolor = truecolor;
                break;
            }
            case 'R': {
                int cols, rows;
                if (scanf("%d %d", &cols, &rows) != 2) fail("bad R");
                printf("resize %s\n", cells_status_string(cells_resize(&cells, cols, rows)));
                break;
            }
            case 'K': {
                int cols, rows;
                if (scanf("%d %d", &cols, &rows) != 2) fail("bad K");
                cells_keep_out_of(&cells, cols, rows);
                break;
            }
            case 'V':
                cells_invalidate(&cells);
                break;
            case 'C': {
                int width, height;
                if (scanf("%d %d", &width, &height) != 2 || getchar() != '\n') fail("bad C");
                png_image_free(&canvas);
                if (width > 0) {
                    if (png_image_alloc(&canvas, width, height) != PNG_OK) fail("canvas");
                    size_t length = (size_t)width * (size_t)height * 4;
                    if (fread(canvas.pixels, 1, length, stdin) != length) fail("short canvas");
                }
                break;
            }
            case 'D': {
                int style, cell_width, cell_height;
                if (scanf("%d %d %d", &style, &cell_width, &cell_height) != 3) fail("bad D");
                cells_read(&cells, style_of(style), &canvas, cell_width, cell_height);
                break;
            }
            case 'E': {
                cells_status_t status = cells_emit(&cells);
                printf("emit %s %zu %zu\ntext ", cells_status_string(status), cells.length,
                       cells.capacity);
                if (cells.text != NULL) print_escaped(cells.text, cells.length);
                putchar('\n');
                break;
            }
            case 'P': {
                int style, cell_width, cell_height, r, g, b;
                if (scanf("%d %d %d %d %d %d", &style, &cell_width, &cell_height, &r, &g, &b) != 6)
                    fail("bad P");
                const uint8_t ground[3] = {(uint8_t)r, (uint8_t)g, (uint8_t)b};
                png_image_t picture = {0, 0, NULL};
                cells_status_t status =
                    cells_paint(&cells, style_of(style), &picture, cell_width, cell_height, ground);
                printf("paint %s", cells_status_string(status));
                if (status == CELLS_OK) {
                    printf(" %d %d\n", picture.width, picture.height);
                    for (int y = 0; y < picture.height; y++) {
                        const uint8_t *line = &picture.pixels[(size_t)y * (size_t)picture.width * 4];
                        for (size_t i = 0; i < (size_t)picture.width * 4; i++)
                            printf("%02x", line[i]);
                        putchar('\n');
                    }
                } else {
                    putchar('\n');
                }
                png_image_free(&picture);
                break;
            }
            case 'S':
                printf("state %d %d %d %d %d %d\n", cells.cols, cells.rows, cells.draw_everything,
                       cells.keep_cols, cells.keep_rows, cells.truecolor);
                print_grid("now", cells.now);
                print_grid("before", cells.before);
                break;
            case 'G': {
                int grid;
                unsigned long index;
                unsigned glyph, fr, fg, fb, br, bg, bb, has_fg, has_bg;
                if (scanf("%d %lu %u %u %u %u %u %u %u %u %u", &grid, &index, &glyph, &fr, &fg, &fb,
                          &br, &bg, &bb, &has_fg, &has_bg) != 11)
                    fail("bad G");
                cell_t *cells_of = grid ? cells.before : cells.now;
                if (cells_of != NULL && index < (unsigned long)cells.cols * (unsigned long)cells.rows)
                    cells_of[index] = (cell_t){glyph,
                                               {(uint8_t)fr, (uint8_t)fg, (uint8_t)fb},
                                               {(uint8_t)br, (uint8_t)bg, (uint8_t)bb},
                                               (uint8_t)has_fg,
                                               (uint8_t)has_bg};
                break;
            }
            default:
                fail("unknown command");
        }
    }
    if (initialised) cells_destroy(&cells);
    png_image_free(&canvas);
    return 0;
}

/*
 * Observes the pinned cbirds kitty_graphics.c: runs a script of calls read
 * from stdin against one kitty_graphics_t and prints what each did on stderr,
 * for tests/kitty_oracle.rs to compare with the Rust translation. It only
 * calls the reference's functions and prints their results.
 *
 * usage: kitty_oracle MODE, where MODE picks the output descriptor:
 *   stdout  standard output (the flushed bytes are the oracle's stdout)
 *   bad     INT_MAX, which no process has open
 *   epipe   a pipe whose read end is closed (SIGPIPE ignored)
 *   full    a pipe filled until EAGAIN, as the C test does, then made
 *           blocking again; R drains it
 *
 * Script: one command a line, fields separated by spaces.
 *   U id len\n<len bytes>          kitty_graphics_upload_png
 *   P image placement row col x y z kitty_graphics_place
 *   D image placement              kitty_graphics_delete_placement
 *   A                              kitty_graphics_delete_all_placements
 *   X image                        kitty_graphics_delete_image
 *   T row col len\n<len bytes>     kitty_graphics_write_text (the bytes, NUL
 *                                  terminated, as a C string)
 *   W len\n<len bytes>             kitty_graphics_write_raw
 *   B, E                           begin, end synchronized update
 *   L                              graphics.length = 0, as the benchmark does
 *   F, N                           flush, flush_nonblocking
 *   R n                            read up to n bytes from the full pipe
 *   O on                           sets (1) or clears (0) O_NONBLOCK on the
 *                                  output descriptor from outside
 *   Q                              the queued bytes
 *
 * Transcript (stderr), after each command other than R and Q:
 *   <command> <status string> <length> <capacity>[ errno=<errno>][ nonblock=<0|1>]
 * with errno after ERR_IO and AGAIN, and nonblock after F and N.
 */
#define _POSIX_C_SOURCE 200809L
#include "kitty_graphics.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void fail(const char *why) {
    fprintf(stderr, "kitty_oracle: %s\n", why);
    exit(2);
}

/* Reads "len\n" then len bytes into a fresh NUL-terminated buffer. */
static char *read_blob(size_t *length) {
    unsigned long wanted;
    if (scanf("%lu", &wanted) != 1 || getchar() != '\n') fail("bad blob length");
    char *blob = malloc(wanted + 1);
    if (blob == NULL) fail("memory");
    if (fread(blob, 1, wanted, stdin) != wanted) fail("short blob");
    blob[wanted] = '\0';
    *length = wanted;
    return blob;
}

static void print_escaped(const char *bytes, size_t length) {
    for (size_t i = 0; i < length; i++) {
        unsigned char c = (unsigned char)bytes[i];
        if (c >= 0x20 && c < 0x7f && c != '\\')
            fputc(c, stderr);
        else
            fprintf(stderr, "\\x%02x", c);
    }
}

int main(int argc, char **argv) {
    if (argc != 2) fail("usage: kitty_oracle stdout|bad|epipe|full");
    const char *mode = argv[1];
    int fd = -1, drain_fd = -1;
    signal(SIGPIPE, SIG_IGN);
    if (strcmp(mode, "stdout") == 0) {
        fd = STDOUT_FILENO;
    } else if (strcmp(mode, "bad") == 0) {
        fd = INT_MAX;
    } else if (strcmp(mode, "epipe") == 0 || strcmp(mode, "full") == 0) {
        int descriptors[2];
        if (pipe(descriptors) != 0) fail("pipe");
        fd = descriptors[1];
        drain_fd = descriptors[0];
        if (strcmp(mode, "epipe") == 0) {
            close(drain_fd);
            drain_fd = -1;
        } else {
            char fill[4096] = {0};
            int flags = fcntl(fd, F_GETFL);
            if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) != 0) fail("fcntl");
            while (write(fd, fill, sizeof(fill)) > 0) {
            }
            if (errno != EAGAIN && errno != EWOULDBLOCK) fail("fill");
            if (fcntl(fd, F_SETFL, flags) != 0) fail("fcntl");
        }
    } else {
        fail("unknown mode");
    }

    kitty_graphics_t graphics;
    if (kitty_graphics_init(&graphics, fd) != KITTY_GRAPHICS_OK) fail("init");

    char command;
    while (scanf(" %c", &command) == 1) {
        kitty_graphics_status_t status = KITTY_GRAPHICS_OK;
        int flushed = 0;
        switch (command) {
            case 'U': {
                unsigned id;
                size_t length;
                if (scanf("%u", &id) != 1) fail("bad U");
                char *blob = read_blob(&length);
                status = kitty_graphics_upload_png(&graphics, id, (const uint8_t *)blob, length);
                free(blob);
                break;
            }
            case 'P': {
                kitty_graphics_placement_t placement;
                if (scanf("%u %u %d %d %d %d %d", &placement.image_id, &placement.placement_id,
                          &placement.row, &placement.column, &placement.x_offset,
                          &placement.y_offset, &placement.z_index) != 7)
                    fail("bad P");
                status = kitty_graphics_place(&graphics, &placement);
                break;
            }
            case 'D': {
                unsigned image, placement;
                if (scanf("%u %u", &image, &placement) != 2) fail("bad D");
                status = kitty_graphics_delete_placement(&graphics, image, placement);
                break;
            }
            case 'A':
                status = kitty_graphics_delete_all_placements(&graphics);
                break;
            case 'X': {
                unsigned image;
                if (scanf("%u", &image) != 1) fail("bad X");
                status = kitty_graphics_delete_image(&graphics, image);
                break;
            }
            case 'T': {
                int row, column;
                size_t length;
                if (scanf("%d %d", &row, &column) != 2) fail("bad T");
                char *blob = read_blob(&length);
                status = kitty_graphics_write_text(&graphics, row, column, blob);
                free(blob);
                break;
            }
            case 'W': {
                size_t length;
                char *blob = read_blob(&length);
                status = kitty_graphics_write_raw(&graphics, blob, length);
                free(blob);
                break;
            }
            case 'B':
                status = kitty_graphics_begin_synchronized_update(&graphics);
                break;
            case 'E':
                status = kitty_graphics_end_synchronized_update(&graphics);
                break;
            case 'L':
                graphics.length = 0;
                break;
            case 'F':
                status = kitty_graphics_flush(&graphics);
                flushed = 1;
                break;
            case 'N':
                status = kitty_graphics_flush_nonblocking(&graphics);
                flushed = 1;
                break;
            case 'R': {
                unsigned long wanted;
                static char drained[1 << 17];
                if (scanf("%lu", &wanted) != 1 || wanted > sizeof(drained) || drain_fd < 0)
                    fail("bad R");
                ssize_t got = read(drain_fd, drained, wanted);
                fprintf(stderr, "read %ld\n", (long)got);
                fprintf(stderr, "drained ");
                if (got > 0) print_escaped(drained, (size_t)got);
                fputc('\n', stderr);
                continue;
            }
            case 'O': {
                int on;
                if (scanf("%d", &on) != 1) fail("bad O");
                int flags = fcntl(fd, F_GETFL);
                if (flags < 0 ||
                    fcntl(fd, F_SETFL, on ? flags | O_NONBLOCK : flags & ~O_NONBLOCK) != 0)
                    fail("fcntl");
                continue;
            }
            case 'Q':
                fprintf(stderr, "buffer ");
                if (graphics.length > 0) print_escaped(graphics.buffer, graphics.length);
                fputc('\n', stderr);
                continue;
            default:
                fail("unknown command");
        }
        int error = errno;
        fprintf(stderr, "%c %s %zu %zu", command, kitty_graphics_status_string(status),
                graphics.length, graphics.capacity);
        if (status == KITTY_GRAPHICS_ERR_IO || status == KITTY_GRAPHICS_AGAIN)
            fprintf(stderr, " errno=%d", error);
        if (flushed) {
            int flags = fcntl(fd, F_GETFL);
            fprintf(stderr, " nonblock=%d", flags < 0 ? -1 : (flags & O_NONBLOCK) != 0);
        }
        fputc('\n', stderr);
    }
    kitty_graphics_destroy(&graphics);
    return 0;
}

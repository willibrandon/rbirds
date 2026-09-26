/*
 * Observation program for the GIF differential tests (tests/gif_oracle.rs).
 *
 * Built by tests/support/oracle.rs together with the unmodified reference
 * gif.c and png.c. It opens PATH with gif_open, hands every frame record on
 * stdin to gif_add_frame, closes with gif_close, and prints each status:
 *
 *     open STATUS
 *     frame STATUS          (one line a record)
 *     close STATUS BYTES FRAMES
 *
 * STATUS is the gif_status_t value. When gif_open fails nothing follows its
 * line. A frame record is, little endian,
 *     i32 width, i32 height, u8 present, width * height * 4 bytes if present
 * where present == 0 is a frame whose pixels are NULL.
 *
 * usage: gif_oracle PATH WIDTH HEIGHT DELAY < frames
 */

#include "gif.h"
#include "png.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void die(const char *why) {
    fprintf(stderr, "gif_oracle: %s\n", why);
    exit(3);
}

static int read_exact(void *buffer, size_t length) {
    size_t got = fread(buffer, 1, length, stdin);
    if (got == length) return 1;
    if (got == 0 && feof(stdin)) return 0;
    die("truncated frame record");
    return 0;
}

static int32_t le32(const uint8_t *b) {
    return (int32_t)((uint32_t)b[0] | ((uint32_t)b[1] << 8) | ((uint32_t)b[2] << 16) |
                     ((uint32_t)b[3] << 24));
}

int main(int argc, char **argv) {
    if (argc != 5) die("usage: gif_oracle PATH WIDTH HEIGHT DELAY < frames");
    int width = atoi(argv[2]), height = atoi(argv[3]), delay = atoi(argv[4]);

    gif_writer_t *writer = NULL;
    gif_status_t status = gif_open(&writer, argv[1], width, height, delay);
    printf("open %d\n", (int)status);
    if (status != GIF_OK) {
        /* A writer handed back with a failed header write is closed quietly. */
        if (writer != NULL) gif_close(writer, NULL, NULL);
        return fflush(stdout) == 0 ? 0 : 3;
    }

    uint8_t header[9];
    while (read_exact(header, sizeof(header))) {
        png_image_t frame = {le32(header), le32(header + 4), NULL};
        if (header[8]) {
            size_t length = (size_t)frame.width * (size_t)frame.height * 4;
            frame.pixels = (uint8_t *)malloc(length ? length : 1);
            if (frame.pixels == NULL) die("out of memory reading a frame");
            if (length > 0 && !read_exact(frame.pixels, length)) die("truncated frame record");
        }
        printf("frame %d\n", (int)gif_add_frame(writer, &frame));
        free(frame.pixels);
    }

    size_t bytes = 0;
    int frames = 0;
    status = gif_close(writer, &bytes, &frames);
    printf("close %d %zu %d\n", (int)status, bytes, frames);
    return fflush(stdout) == 0 ? 0 : 3;
}

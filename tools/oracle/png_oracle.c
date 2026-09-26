/*
 * Observation program for the PNG differential tests (tests/png_oracle.rs).
 *
 * Built by tests/support/oracle.rs together with the unmodified reference
 * png.c. It reads a stream of binary records from stdin, hands each to the
 * public png.h API and writes what came back to stdout. It computes nothing
 * of its own except the sprite pipeline's wing squash, copied from boids.c
 * (squash_wings) because that function is static there.
 *
 * All integers are little endian. An image record is
 *     i32 width, i32 height, u8 present, width * height * 4 bytes if present
 * where present == 0 is an image whose pixels are NULL. A status is one byte,
 * the png_status_t value.
 *
 * usage: png_oracle decode | encode | resize | rotate | rotate_resize | tint | sprites
 *
 *   decode         in: u32 length, bytes         out: status [image]
 *   encode         in: image                     out: status [u32 length, bytes]
 *   resize         in: image, i32 w, i32 h       out: status [image]
 *   rotate         in: image, u64 radians bits   out: status [image]
 *   rotate_resize  in: image, u64 bits, i32 w, i32 h   out: status [image]
 *   tint           in: image, u8 r, g, b, mode   out: image
 *   sprites        in: image source, i32 size    out: see run_sprites
 */

#include "png.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

enum { FRAME_ANGLE = 6, ROTATION_FRAMES = 60, SPRITE_SUPERSAMPLE = 6, SPRITE_WORK_MAX = 256 };

static void die(const char *why) {
    fprintf(stderr, "png_oracle: %s\n", why);
    exit(3);
}

/* 1 when the whole buffer was read, 0 on end of input before its first byte. */
static int read_exact(void *buffer, size_t length) {
    size_t got = fread(buffer, 1, length, stdin);
    if (got == length) return 1;
    if (got == 0 && feof(stdin)) return 0;
    die("truncated input record");
    return 0;
}

static void need(void *buffer, size_t length) {
    if (length > 0 && !read_exact(buffer, length)) die("truncated input record");
}

static uint32_t get_u32(void) {
    uint8_t b[4];
    need(b, 4);
    return (uint32_t)b[0] | ((uint32_t)b[1] << 8) | ((uint32_t)b[2] << 16) | ((uint32_t)b[3] << 24);
}

static int32_t get_i32(void) {
    return (int32_t)get_u32();
}

static double get_f64(void) {
    uint8_t b[8];
    uint64_t bits = 0;
    double value;
    need(b, 8);
    for (int i = 7; i >= 0; i--) bits = (bits << 8) | b[i];
    memcpy(&value, &bits, sizeof(value));
    return value;
}

static void put(const void *data, size_t length) {
    if (length > 0 && fwrite(data, 1, length, stdout) != length) die("cannot write output");
}

static void put_u8(unsigned value) {
    uint8_t b = (uint8_t)value;
    put(&b, 1);
}

static void put_u32(uint32_t value) {
    uint8_t b[4] = {(uint8_t)value, (uint8_t)(value >> 8), (uint8_t)(value >> 16),
                    (uint8_t)(value >> 24)};
    put(b, 4);
}

static void put_i32(int32_t value) {
    put_u32((uint32_t)value);
}

/* Reads an image record, 0 at the end of the input. */
static int get_image(png_image_t *image) {
    uint8_t header[9];
    if (!read_exact(header, sizeof(header))) return 0;
    image->width = (int)((uint32_t)header[0] | ((uint32_t)header[1] << 8) |
                         ((uint32_t)header[2] << 16) | ((uint32_t)header[3] << 24));
    image->height = (int)((uint32_t)header[4] | ((uint32_t)header[5] << 8) |
                          ((uint32_t)header[6] << 16) | ((uint32_t)header[7] << 24));
    image->pixels = NULL;
    if (header[8]) {
        size_t length = (size_t)image->width * (size_t)image->height * 4;
        image->pixels = (uint8_t *)malloc(length ? length : 1);
        if (image->pixels == NULL) die("out of memory reading an image");
        need(image->pixels, length);
    }
    return 1;
}

static void put_image(const png_image_t *image) {
    put_i32(image->width);
    put_i32(image->height);
    put_u8(image->pixels != NULL);
    if (image->pixels != NULL) put(image->pixels, (size_t)image->width * (size_t)image->height * 4);
}

static void run_decode(void) {
    uint8_t length_bytes[4];
    while (read_exact(length_bytes, 4)) {
        uint32_t length = (uint32_t)length_bytes[0] | ((uint32_t)length_bytes[1] << 8) |
                          ((uint32_t)length_bytes[2] << 16) | ((uint32_t)length_bytes[3] << 24);
        uint8_t *data = (uint8_t *)malloc(length ? length : 1);
        if (data == NULL) die("out of memory reading a file");
        need(data, length);
        png_image_t out = {0, 0, NULL};
        png_status_t status = png_decode(data, length, &out);
        put_u8(status);
        if (status == PNG_OK) put_image(&out);
        png_image_free(&out);
        free(data);
    }
}

static void run_encode(void) {
    png_image_t image;
    while (get_image(&image)) {
        uint8_t *encoded = NULL;
        size_t length = 0;
        png_status_t status = png_encode(&image, &encoded, &length);
        put_u8(status);
        if (status == PNG_OK) {
            put_u32((uint32_t)length);
            put(encoded, length);
        }
        free(encoded);
        free(image.pixels);
    }
}

static void run_resize(void) {
    png_image_t image;
    while (get_image(&image)) {
        int width = get_i32(), height = get_i32();
        png_image_t out = {0, 0, NULL};
        png_status_t status = png_resize(&image, width, height, &out);
        put_u8(status);
        if (status == PNG_OK) put_image(&out);
        png_image_free(&out);
        free(image.pixels);
    }
}

static void run_rotate(void) {
    png_image_t image;
    while (get_image(&image)) {
        double radians = get_f64();
        png_image_t out = {0, 0, NULL};
        png_status_t status = png_rotate(&image, radians, &out);
        put_u8(status);
        if (status == PNG_OK) put_image(&out);
        png_image_free(&out);
        free(image.pixels);
    }
}

static void run_rotate_resize(void) {
    png_image_t image;
    while (get_image(&image)) {
        double radians = get_f64();
        int width = get_i32(), height = get_i32();
        png_image_t out = {0, 0, NULL};
        png_status_t status = png_rotate_resize(&image, radians, width, height, &out);
        put_u8(status);
        if (status == PNG_OK) put_image(&out);
        png_image_free(&out);
        free(image.pixels);
    }
}

static void run_tint(void) {
    png_image_t image;
    while (get_image(&image)) {
        uint8_t args[4];
        need(args, 4);
        png_tint(&image, args[0], args[1], args[2],
                 args[3] ? PNG_TINT_REPLACE : PNG_TINT_MULTIPLY);
        put_image(&image);
        free(image.pixels);
    }
}

/* boids.c squash_wings, verbatim but for the name. */
static png_status_t squash_wings(const png_image_t *square, double span, png_image_t *out) {
    int height = (int)(square->height * span + 0.5);
    if (height < 1) height = 1;
    png_image_t narrow = {0, 0, NULL};
    png_status_t status = png_resize(square, square->width, height, &narrow);
    if (status == PNG_OK) status = png_image_alloc(out, square->width, square->height);
    if (status == PNG_OK) {
        int top = (square->height - height) / 2;
        for (int y = 0; y < height; y++)
            memcpy(out->pixels + ((size_t)(top + y) * (size_t)out->width) * 4,
                   narrow.pixels + (size_t)y * (size_t)narrow.width * 4, (size_t)narrow.width * 4);
    }
    png_image_free(&narrow);
    return status;
}

/*
 * The geometry half of boids.c rasterise_geometry, for one bird size: the
 * source resized to the working square, then for the three wing spans (1.0,
 * 0.72, 0.45) the squashed square and its ROTATION_FRAMES rotations.
 *
 * out: status, square image; then per span < 1: i32 squashed height (the
 * same expression as squash_wings), status, image; then per span, per frame:
 * status, image.
 */
static void run_sprites(void) {
    static const double SPANS[3] = {1.0, 0.72, 0.45};
    png_image_t source;
    while (get_image(&source)) {
        int size = get_i32();
        int work = size * SPRITE_SUPERSAMPLE;
        if (work > SPRITE_WORK_MAX) work = SPRITE_WORK_MAX;
        if (work > source.width) work = source.width;

        png_image_t square = {0, 0, NULL};
        png_status_t status = png_resize(&source, work, work, &square);
        put_u8(status);
        if (status != PNG_OK) {
            free(source.pixels);
            continue;
        }
        put_image(&square);

        png_image_t geometry[3] = {{0, 0, NULL}, {0, 0, NULL}, {0, 0, NULL}};
        geometry[0] = square;
        for (int k = 1; k < 3; k++) {
            int height = (int)(square.height * SPANS[k] + 0.5);
            if (height < 1) height = 1;
            put_i32(height);
            status = squash_wings(&square, SPANS[k], &geometry[k]);
            put_u8(status);
            if (status != PNG_OK) die("squash failed");
            put_image(&geometry[k]);
        }
        for (int k = 0; k < 3; k++) {
            for (int i = 0; i < ROTATION_FRAMES; i++) {
                png_image_t base = {0, 0, NULL};
                status = png_rotate_resize(&geometry[k], i * FRAME_ANGLE * M_PI / 180.0, size,
                                           size, &base);
                put_u8(status);
                if (status == PNG_OK) put_image(&base);
                png_image_free(&base);
            }
        }
        for (int k = 0; k < 3; k++) png_image_free(&geometry[k]);
        free(source.pixels);
    }
}

int main(int argc, char **argv) {
    if (argc != 2) die("usage: png_oracle COMMAND < records");
    const char *command = argv[1];
    if (strcmp(command, "decode") == 0)
        run_decode();
    else if (strcmp(command, "encode") == 0)
        run_encode();
    else if (strcmp(command, "resize") == 0)
        run_resize();
    else if (strcmp(command, "rotate") == 0)
        run_rotate();
    else if (strcmp(command, "rotate_resize") == 0)
        run_rotate_resize();
    else if (strcmp(command, "tint") == 0)
        run_tint();
    else if (strcmp(command, "sprites") == 0)
        run_sprites();
    else
        die("unknown command");
    if (fflush(stdout) != 0) die("cannot write output");
    return 0;
}

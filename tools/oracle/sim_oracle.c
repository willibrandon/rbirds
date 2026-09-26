/*
 * Simulation oracle: runs a scripted scenario through the unmodified cbirds
 * boids.c and prints its state, for tests/sim_oracle.rs to compare with the
 * Rust port field by field.
 *
 * boids.c is included with its main renamed, exactly as tests/boids_test.c
 * includes it, so every static function and global is the reference's own.
 * This file only sets inputs, calls those functions in the order the named
 * reference path calls them, and prints what they left behind. It contains
 * no flocking arithmetic of its own.
 *
 * Script, one command a line on stdin (# starts a comment):
 *   screen COLS ROWS WIDTH HEIGHT    apply_screen_size
 *   set FIELD VALUE                  an integer setting (see set_field)
 *   notches | defaults | preset K    apply_notches / apply_preset_defaults / apply_preset
 *   seconds BITS                     set_frame_seconds(the double with these bits)
 *   seed N                           seed_random(N)
 *   alloc N                          birds and snapshot arrays of N, zeroed
 *   init | intro | hawks             initialize_birds / begin_the_intro / place_hawks
 *   grid                             spatial_grid_prepare for the screen and config.birds
 *   record F FPS_BITS                one frame of the recording loop
 *   bench                            one frame of the benchmark loop, without drawing
 *   fly                              snapshot, build, fly
 *   keys HEX                         handle_input over these bytes (- for none)
 *   theme R G B R G B                ramp_between(accent, background), theme known
 *   resize N                         config.birds = N; resize_the_flock
 *   sprites                          prepare_text_renderer (or rasterise for kitty)
 *   render                           queue_render_frame, bytes printed as hex
 *   live_begin SEC NSEC              the live loop's own state, started at that time
 *   live HEX SEC NSEC COLS ROWS XPIXELS YPIXELS
 *                                    one pass of main's loop body, from handle_input
 *                                    to the queued frame, with the monotonic clock
 *                                    and window given rather than read; the frame's
 *                                    bytes printed as hex
 *   stats MICROSECONDS BYTES         main's per-second panel statistics for a frame
 *   dump | digest                    the whole state, or its FNV-1a 64 digest
 *
 * `live` and `stats` repeat main's loop body line for line, because main
 * itself cannot be entered piecemeal; that is orchestration, and every
 * function it calls is the reference's.
 *
 * Output format "rbirds-sim-trace 1": integers in decimal, doubles as 16 hex
 * digits of their IEEE bits, floats as 8.
 */
#define main cbirds_application_main
#include "boids.c"
#undef main

#include <fcntl.h>

#include "state_dump.h"

static double from_bits(const char *hex) {
    uint64_t b = strtoull(hex, NULL, 16);
    double value;
    memcpy(&value, &b, sizeof(value));
    return value;
}

static kitty_graphics_t graphics;
static int graphics_ready;

static uint64_t fnv1a(const char *text, size_t length) {
    uint64_t hash = 1469598103934665603ull;
    for (size_t i = 0; i < length; i++) hash = (hash ^ (unsigned char)text[i]) * 1099511628211ull;
    return hash;
}

static void flush_out(void) {
    fwrite(out_text, 1, out_length, stdout);
    out_length = 0;
}

static int set_field(const char *field, long value) {
    int v = (int)value;
    if (strcmp(field, "birds") == 0) config.birds = v;
    else if (strcmp(field, "size") == 0) config.bird_size = v;
    else if (strcmp(field, "palette") == 0) config.palette = v;
    else if (strcmp(field, "flocks") == 0) config.flocks = v;
    else if (strcmp(field, "trails") == 0) config.trails = v;
    else if (strcmp(field, "hawks") == 0) config.hawks = v;
    else if (strcmp(field, "shape") == 0) config.shape = v;
    else if (strcmp(field, "turning") == 0) config.turning_notch = v;
    else if (strcmp(field, "boundary") == 0) config.boundary_notch = v;
    else if (strcmp(field, "separation") == 0) config.separation_notch = v;
    else if (strcmp(field, "alignment") == 0) config.alignment_notch = v;
    else if (strcmp(field, "vision") == 0) config.vision_notch = v;
    else if (strcmp(field, "pace") == 0) config.pace_notch = v;
    else if (strcmp(field, "avoid") == 0) config.avoid_notch = v;
    else if (strcmp(field, "legend") == 0) legend_enabled = v;
    else if (strcmp(field, "render") == 0) render_mode = v;
    else if (strcmp(field, "deep") == 0) deep_look = v;
    else if (strcmp(field, "rain") == 0) the_rain_is_falling = v;
    else if (strcmp(field, "paused") == 0) paused = v;
    else if (strcmp(field, "hawk_sets") == 0) hawk_sets_built = v;
    else if (strcmp(field, "preset_index") == 0) requested_preset = v;
    else if (strcmp(field, "truecolor") == 0) text_cells.truecolor = v;
    else return 0;
    return 1;
}

static int handle_keys(const char *hex) {
    static char bytes[1 << 16];
    size_t length = 0;
    if (strcmp(hex, "-") != 0)
        for (const char *c = hex; c[0] && c[1] && length < sizeof(bytes); c += 2) {
            char pair[3] = {c[0], c[1], 0};
            bytes[length++] = (char)strtol(pair, NULL, 16);
        }
    int descriptors[2];
    if (pipe(descriptors) != 0) abort();
    /* A pipe holds far more than one read takes; handle_input reads at most
     * INPUT_BUFFER_SIZE, and the rest is dropped with the pipe, as a burst
     * longer than one read would wait for the next frame's read live. */
    if (length > 0 && write(descriptors[1], bytes, length) != (ssize_t)length) abort();
    close(descriptors[1]);
    int saved = dup(STDIN_FILENO);
    dup2(descriptors[0], STDIN_FILENO);
    close(descriptors[0]);
    /* An empty pipe is end of file: a zero-byte read, as an idle raw
     * terminal's read is. */
    int result = handle_input();
    dup2(saved, STDIN_FILENO);
    close(saved);
    return result;
}

static void emit_buffer(const char *label, int status) {
    emit("%s %d %zu ", label, status, graphics.length);
    for (size_t i = 0; i < graphics.length; i++) emit("%02x", (unsigned char)graphics.buffer[i]);
    emit("\n");
}

static void ensure_graphics(void) {
    if (!graphics_ready && kitty_graphics_init(&graphics, STDOUT_FILENO) != KITTY_GRAPHICS_OK)
        abort();
    graphics_ready = 1;
}

static int live_birds;
static double leaving;
static struct timespec started, previous_frame;

int main(void) {
    static char line[1 << 17];
    trig_lookup_init();
    name_the_palettes();
    name_the_presets();
    name_the_shapes();
    while (fgets(line, sizeof(line), stdin) != NULL) {
        char *word[16] = {0};
        int words = 0;
        if (line[0] == '#') continue;
        for (char *token = strtok(line, " \t\n"); token != NULL && words < 16;
             token = strtok(NULL, " \t\n"))
            word[words++] = token;
        if (words == 0) continue;
        for (int i = words; i < 16; i++) word[i] = "";
        const char *command = word[0], *a = word[1], *b = word[2], *c = word[3], *d = word[4];
        if (strcmp(command, "screen") == 0) {
            apply_screen_size(atoi(a), atoi(b), atoi(c), atoi(d));
        } else if (strcmp(command, "set") == 0) {
            if (!set_field(a, strtol(b, NULL, 10))) {
                fprintf(stderr, "unknown field %s\n", a);
                return 2;
            }
        } else if (strcmp(command, "notches") == 0) {
            apply_notches();
        } else if (strcmp(command, "defaults") == 0) {
            apply_preset_defaults();
        } else if (strcmp(command, "preset") == 0) {
            apply_preset(atoi(a));
        } else if (strcmp(command, "seconds") == 0) {
            set_frame_seconds(from_bits(a));
        } else if (strcmp(command, "seed") == 0) {
            seed_random((unsigned)strtoul(a, NULL, 10));
        } else if (strcmp(command, "alloc") == 0) {
            free(birds);
            free(snapshot);
            allocated = atoi(a);
            birds = calloc((size_t)allocated, sizeof(*birds));
            snapshot = calloc((size_t)allocated, sizeof(*snapshot));
        } else if (strcmp(command, "init") == 0) {
            initialize_birds(birds);
        } else if (strcmp(command, "hawks") == 0) {
            place_hawks();
        } else if (strcmp(command, "intro") == 0) {
            begin_the_intro();
        } else if (strcmp(command, "grid") == 0) {
            if (!grid_ready) spatial_grid_init(&grid, SPATIAL_CELL_SIZE);
            grid_ready = 1;
            emit("grid %d\n", spatial_grid_prepare(&grid, screen.width, screen.height, config.birds));
        } else if (strcmp(command, "record") == 0) {
            /* run_recording's loop body. */
            double fps = from_bits(b);
            clock_state.frame = atoi(a);
            clock_state.seconds = (double)atoi(a) / fps;
            if (formation.writing && formation.until >= 0 && clock_state.seconds >= formation.until)
                formation_clear();
            maybe_drift();
            memcpy(snapshot, birds, sizeof(*birds) * (size_t)config.birds);
            spatial_grid_build(&grid, config.birds, read_bird_position, snapshot);
            fly(birds, snapshot, &grid);
        } else if (strcmp(command, "bench") == 0) {
            /* run_benchmark's loop body, less the drawing. */
            memcpy(snapshot, birds, sizeof(*birds) * (size_t)config.birds);
            spatial_grid_build(&grid, config.birds, read_bird_position, snapshot);
            hunt(snapshot);
            if (!paused || step_once) {
                step_once = 0;
                fly(birds, snapshot, &grid);
            }
        } else if (strcmp(command, "fly") == 0) {
            memcpy(snapshot, birds, sizeof(*birds) * (size_t)config.birds);
            spatial_grid_build(&grid, config.birds, read_bird_position, snapshot);
            fly(birds, snapshot, &grid);
        } else if (strcmp(command, "clock") == 0) {
            clock_state.frame = atol(a);
            clock_state.seconds = from_bits(b);
        } else if (strcmp(command, "keys") == 0) {
            emit("keys %d\n", handle_keys(a));
        } else if (strcmp(command, "theme") == 0) {
            uint8_t accent[3] = {(uint8_t)atoi(a), (uint8_t)atoi(b), (uint8_t)atoi(c)};
            uint8_t ground[3] = {(uint8_t)atoi(d), (uint8_t)atoi(word[5]), (uint8_t)atoi(word[6])};
            ramp_between(accent, ground);
            theme_is_known = 1;
        } else if (strcmp(command, "resize") == 0) {
            int to = atoi(a);
            int from = config.birds;
            config.birds = to;
            if (!resize_the_flock(&birds, &snapshot, from, to)) abort();
            allocated = to;
        } else if (strcmp(command, "sprites") == 0) {
            int ok = drawing_with_text() ? prepare_text_renderer()
                                         : rasterise_sprites(text_sprites) == PNG_OK;
            emit("sprites %d\n", ok);
        } else if (strcmp(command, "render") == 0) {
            ensure_graphics();
            graphics.length = 0;
            emit_buffer("render", (int)queue_render_frame(&graphics, birds));
        } else if (strcmp(command, "live_begin") == 0) {
            ensure_graphics();
            started.tv_sec = atol(a);
            started.tv_nsec = atol(b);
            previous_frame = started;
            live_birds = config.birds;
            leaving = 0;
        } else if (strcmp(command, "live") == 0) {
            /* main's loop body, from handle_input to the queued frame. */
            ensure_graphics();
            graphics.length = 0;
            if (!handle_keys(a) && leaving <= 0) {
                leaving = (double)OUTRO_FRAMES_AT_SIXTY / FRAME_RATE;
                formation_clear();
            }
            struct timespec frame_start = {atol(b), atol(c)};
            set_frame_seconds(elapsed_seconds(&previous_frame, &frame_start));
            previous_frame = frame_start;
            if (leaving > 0) {
                leaving -= frame_seconds;
                if (leaving <= 0) {
                    emit("live over\n");
                    flush_out();
                    continue;
                }
            }
            clock_state.frame++;
            clock_state.seconds = (double)(frame_start.tv_sec - started.tv_sec) +
                                  (double)(frame_start.tv_nsec - started.tv_nsec) / 1e9;
            if (formation.writing && formation.until >= 0 && clock_state.seconds >= formation.until)
                formation_clear();
            maybe_drift();
            apply_screen_size(atoi(d), atoi(word[5]), atoi(word[6]), atoi(word[7]));
            if (spatial_grid_prepare(&grid, screen.width, screen.height, config.birds) != 0) abort();
            if (population_changed) {
                population_changed = 0;
                if (resize_the_flock(&birds, &snapshot, live_birds, config.birds))
                    live_birds = config.birds;
                else
                    config.birds = live_birds;
                allocated = live_birds;
                if (spatial_grid_prepare(&grid, screen.width, screen.height, config.birds) != 0)
                    abort();
            }
            memcpy(snapshot, birds, sizeof(*birds) * (size_t)config.birds);
            if (spatial_grid_build(&grid, config.birds, read_bird_position, snapshot) != 0) abort();
            if (leaving > 0) fly_away(birds);
            kitty_graphics_status_t status = leaving > 0
                                                 ? queue_render_frame(&graphics, birds)
                                                 : render_frame(&graphics, birds, snapshot, &grid);
            emit_buffer("live", (int)status);
        } else if (strcmp(command, "stats") == 0) {
            /* main's statistics, after a frame's flush. */
            stats.window_ms += (double)atol(a) / 1000.0;
            stats.window_bytes += (double)strtoul(b, NULL, 10);
            stats.counted++;
            if (clock_state.seconds - stats.window_started >= 1.0) {
                double span = clock_state.seconds - stats.window_started;
                stats.frame_ms = stats.window_ms / (double)stats.counted;
                stats.bytes = stats.window_bytes / (double)stats.counted;
                stats.rate = (double)stats.counted / span;
                stats.window_started = clock_state.seconds;
                stats.window_ms = stats.window_bytes = 0;
                stats.counted = 0;
            }
        } else if (strcmp(command, "dump") == 0) {
            dump_state();
        } else if (strcmp(command, "digest") == 0) {
            size_t mark = out_length;
            dump_state();
            uint64_t hash = fnv1a(out_text + mark, out_length - mark);
            out_length = mark;
            emit("digest %016" PRIx64 "\n", hash);
        } else {
            fprintf(stderr, "unknown command %s\n", command);
            return 2;
        }
        flush_out();
    }
    flush_out();
    return 0;
}

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
 *   dump | digest                    the whole state, or its FNV-1a 64 digest
 *
 * Output format "rbirds-sim-trace 1": integers in decimal, doubles as 16 hex
 * digits of their IEEE bits, floats as 8.
 */
#define main cbirds_application_main
#include "boids.c"
#undef main

#include <fcntl.h>
#include <stdarg.h>
#include <inttypes.h>

static char *out_text;
static size_t out_length, out_capacity;

static void emit(const char *format, ...) {
    va_list args;
    for (;;) {
        va_start(args, format);
        size_t room = out_capacity - out_length;
        int wrote = vsnprintf(out_text + out_length, room, format, args);
        va_end(args);
        if (wrote < 0) abort();
        if ((size_t)wrote < room) {
            out_length += (size_t)wrote;
            return;
        }
        out_capacity = out_capacity * 2 + (size_t)wrote + 1024;
        out_text = realloc(out_text, out_capacity);
        if (out_text == NULL) abort();
    }
}

static uint64_t bits(double value) {
    uint64_t b;
    memcpy(&b, &value, sizeof(b));
    return b;
}

static double from_bits(const char *hex) {
    uint64_t b = strtoull(hex, NULL, 16);
    double value;
    memcpy(&value, &b, sizeof(value));
    return value;
}

#define D(value) ((unsigned long long)bits(value))

static bird_t *birds, *snapshot;
static int allocated;
static spatial_grid_t grid;
static int grid_ready;
static kitty_graphics_t graphics;
static int graphics_ready;

static void dump_state(void) {
    emit("rbirds-sim-trace 1\n");
    emit("config birds=%d size=%d palette=%d flocks=%d trails=%d hawks=%d shape=%d turning=%d\n",
         config.birds, config.bird_size, config.palette, config.flocks, config.trails, config.hawks,
         config.shape, config.turning_notch);
    emit("config speed=%016llx base=%016llx pace=%016llx\n", D(config.speed), D(config.base_speed),
         D(config.pace));
    emit("config vision cells=%d radius=%d squared=%d\n", config.vision_cells,
         config.vision_radius, config.vision_radius_squared);
    emit("config weights separation=%016llx alignment=%016llx boundary=%016llx\n",
         D(config.separation), D(config.alignment), D(config.boundary));
    emit("config notches boundary=%d separation=%d alignment=%d vision=%d pace=%d avoid=%d\n",
         config.boundary_notch, config.separation_notch, config.alignment_notch,
         config.vision_notch, config.pace_notch, config.avoid_notch);
    emit("config avoid kinship=%016llx room=%016llx weight=%016llx\n", D(config.avoid_kinship),
         D(config.avoid_room), D(config.avoid_weight));
    emit("screen %d %d %d %d %d %d %d %d %d %d %d\n", screen.width, screen.height, screen.cols,
         screen.rows, screen.cell_width, screen.cell_height, screen.turn_x, screen.turn_y,
         screen.turn_bottom, screen.legend_width, screen.legend_height);
    emit("state legend=%d render=%d deep=%d rain=%d paused=%d step=%d population=%d\n",
         legend_enabled, render_mode, deep_look, the_rain_is_falling, paused, step_once,
         population_changed);
    emit("state frame_seconds=%016llx clock=%ld,%016llx\n", D(frame_seconds), clock_state.frame,
         D(clock_state.seconds));
    emit("state mouse=%d,%016llx,%016llx last_key=%016llx last_drift=%016llx\n", mouse.present,
         D(mouse.x), D(mouse.y), D(last_key_at), D(last_drift_at));
    emit("state konami_at=%d seen=", konami_at);
    for (int i = 0; i < KONAMI_LENGTH; i++) emit("%02x", (unsigned char)konami_seen[i]);
    emit(" preset=%d hawk_sets=%d theme_known=%d theme=", requested_preset, hawk_sets_built,
         theme_is_known);
    for (int i = 0; i < 5; i++)
        emit("%02x%02x%02x", theme_tints[i][0], theme_tints[i][1], theme_tints[i][2]);
    emit("\n");
    emit("rng front=%d rear=%d words=", random_state.front, random_state.rear);
    for (int i = 0; i < RANDOM_WORDS; i++) emit("%08" PRIx32, random_state.word[i]);
    emit("\n");
    emit("formation count=%d writing=%d until=%016llx\n", formation.count, formation.writing,
         D(formation.until));
    for (int i = 0; i < formation.count; i++)
        emit("target %d %016llx %016llx\n", i, D(formation.x[i]), D(formation.y[i]));
    for (int f = 0; f < MAX_FLOCKS; f++)
        emit("flock %d center=%016llx,%016llx home=%016llx,%016llx leash=%016llx\n", f,
             D(flock_center_x[f]), D(flock_center_y[f]), D(flock_home_x[f]), D(flock_home_y[f]),
             D(flock_leash[f]));
    for (int i = 0; i < MAX_HAWKS; i++) {
        const hawk_t *h = &hawks[i];
        emit("hawk %d %016llx %016llx %016llx frame=%d prey=%d commitment=%016llx "
             "passing=%016llx wing=%d clock=%016llx\n",
             i, D(h->x), D(h->y), D(h->direction), h->frame, h->prey, D(h->commitment),
             D(h->passing), h->wing, D(h->wing_clock));
    }
    for (int i = 0; i < allocated; i++) {
        const bird_t *b = &birds[i];
        emit("bird %d %016llx %016llx %016llx frame=%d shade=%d flock=%d layer=%d wing=%d "
             "clock=%016llx glide=%016llx trail=%016llx,%016llx,%016llx/%016llx,%016llx,"
             "%016llx at=%d held=%d\n",
             i, D(b->x), D(b->y), D(b->direction), b->frame, b->shade, b->flock, b->layer, b->wing,
             D(b->wing_clock), D(b->gliding), D(b->trail_x[0]), D(b->trail_x[1]),
             D(b->trail_x[2]), D(b->trail_y[0]), D(b->trail_y[1]), D(b->trail_y[2]),
             b->trail_at, b->trail_held);
    }
    if (grid_ready) {
        emit("grid columns=%d rows=%d cells=%d capacity=%d offsets=", grid.columns, grid.rows,
             grid.cell_count, grid.item_capacity);
        uint64_t hash = 1469598103934665603ull;
        for (int i = 0; i <= grid.cell_count; i++) {
            hash = (hash ^ (uint32_t)grid.offsets[i]) * 1099511628211ull;
        }
        for (int i = 0; i < config.birds && i < grid.item_capacity; i++) {
            hash = (hash ^ (uint32_t)grid.indices[i]) * 1099511628211ull;
        }
        emit("%016" PRIx64 "\n", hash);
    }
}

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

static void feed_keys(const char *hex) {
    char bytes[4096];
    size_t length = 0;
    if (strcmp(hex, "-") != 0)
        for (const char *c = hex; c[0] && c[1] && length < sizeof(bytes); c += 2) {
            char pair[3] = {c[0], c[1], 0};
            bytes[length++] = (char)strtol(pair, NULL, 16);
        }
    int descriptors[2];
    if (pipe(descriptors) != 0) abort();
    if (write(descriptors[1], bytes, length) != (ssize_t)length) abort();
    close(descriptors[1]);
    int saved = dup(STDIN_FILENO);
    dup2(descriptors[0], STDIN_FILENO);
    close(descriptors[0]);
    /* One read, as the live loop makes, of at most INPUT_BUFFER_SIZE bytes;
     * an empty pipe is end of file, which is what an idle raw terminal's
     * zero-byte read looks like to handle_input. */
    int result = handle_input();
    dup2(saved, STDIN_FILENO);
    close(saved);
    emit("keys %d\n", result);
}

int main(void) {
    char line[8192];
    trig_lookup_init();
    name_the_palettes();
    name_the_presets();
    name_the_shapes();
    while (fgets(line, sizeof(line), stdin) != NULL) {
        char command[64] = "", a[4096] = "", b[256] = "", c[256] = "", d[256] = "";
        char e[64] = "", f[64] = "";
        if (line[0] == '#' || line[0] == '\n') continue;
        int n = sscanf(line, "%63s %4095s %255s %255s %255s %63s %63s", command, a, b, c, d, e, f);
        if (n < 1) continue;
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
            feed_keys(a);
        } else if (strcmp(command, "theme") == 0) {
            uint8_t accent[3] = {(uint8_t)atoi(a), (uint8_t)atoi(b), (uint8_t)atoi(c)};
            uint8_t ground[3] = {(uint8_t)atoi(d), (uint8_t)atoi(e), (uint8_t)atoi(f)};
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
            if (!graphics_ready) kitty_graphics_init(&graphics, -1);
            graphics_ready = 1;
            graphics.length = 0;
            kitty_graphics_status_t status = queue_render_frame(&graphics, birds);
            emit("render %d %zu ", (int)status, graphics.length);
            for (size_t i = 0; i < graphics.length; i++)
                emit("%02x", (unsigned char)graphics.buffer[i]);
            emit("\n");
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

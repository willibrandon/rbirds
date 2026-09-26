/*
 * The state dump shared by the oracles: every global of the simulation that
 * boids.c keeps, in the "rbirds-sim-trace 1" format tests/support/sim.rs
 * prints and parses. Include after boids.c. Integers print in decimal,
 * doubles as the 16 hex digits of their IEEE bits.
 */
#ifndef RBIRDS_STATE_DUMP_H
#define RBIRDS_STATE_DUMP_H

#include <inttypes.h>
#include <stdarg.h>

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

#define D(value) ((unsigned long long)bits(value))

static bird_t *birds, *snapshot;
static int allocated;
static spatial_grid_t grid;
static int grid_ready;

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
    emit("stats frame_ms=%016llx bytes=%016llx rate=%016llx counted=%ld started=%016llx "
         "window_ms=%016llx window_bytes=%016llx legend_drawn=%d text_legend=%d\n",
         D(stats.frame_ms), D(stats.bytes), D(stats.rate), stats.counted, D(stats.window_started),
         D(stats.window_ms), D(stats.window_bytes), legend_drawn, text_legend_was_drawn);
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

#endif

/*
 * Oracle for cbirds options.c, driven by rbirds tests/options_oracle.rs.
 *
 * It only observes: the unmodified options_parse, options_usage and
 * options_completion run against copies of the option tables in boids.c
 * (OPTIONS, EXAMPLES and the tagline, with plain targets at boids.c's
 * defaults) and tests/options_test.c (TABLE, EXAMPLES, QUOTED), a synthetic
 * table for the parser's generic corners, and a one row table whose range the
 * caller chooses. It prints what came back.
 *
 * Commands arrive on stdin, one a line, as tokens separated by one space. An
 * argument or other byte string is 'x' followed by its bytes in hex, so any
 * byte but NUL gets through; tables are B (boids.c), T (options_test.c), X
 * (synthetic) and Q (the quoted help):
 *
 *   p TABLE SIZE ARG...                 options_parse, argv {"cbirds", ARG...}
 *   r i|d MINBITS MAXBITS SIZE ARG...   the same for the row "x" with that range
 *   u TABLE EVERYTHING PROGRAM TAGLINE EXAMPLES
 *                                       options_usage; TAGLINE is - (NULL), =
 *                                       (the table's) or a string; EXAMPLES is
 *                                       0 (NULL), 1 (the table's) or 2 (none)
 *   c TABLE SHELL PROGRAM               options_completion
 *
 * Each answer is lines of "field value", closed by "end". Every case starts
 * from the table's defaults; options.c itself keeps no state. Run as
 * `options_oracle argv TABLE SIZE ARG...` it parses the raw arguments the OS
 * delivered, once, in a fresh process.
 */

#include "options.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- boids.c: its constants, targets, OPTIONS, EXAMPLES and tagline ---- */

enum {
    MAX_FLOCKS = 3,
    DEFAULT_TURNING_NOTCH = 8,
    MAX_HAWKS = 4,
    MIN_BIRD_SIZE = 4,
    MAX_BIRD_SIZE = 64,
    MAX_BIRDS = 4096,
    MAX_CAST_FPS = 120,
    MIN_VISION_RADIUS = 12,
    MAX_VISION_RADIUS = 60,
    DEFAULT_VISION_RADIUS = 36,
    LEGEND_BAR_CELLS = 12,
};
#define DEFAULT_NOTCH 4
#define DEFAULT_PACE_NOTCH 1

static struct {
    int birds, bird_size, palette, flocks;
    int trails, hawks, shape;
    int turning_notch;
    int boundary_notch, separation_notch, alignment_notch;
    int pace_notch;
    int avoid_notch;
} config;
static int legend_enabled, render_mode, deep_look, matrix_mode, unlock_fps;
static int requested_perception, requested_seed, requested_preset;
static int frame_limit, bench_frames, record_fps, record_seconds;
static const char *sprite_path, *snapshot_path, *record_path, *requested_record_size;

static const char *const PRESET_NAMES[] = {"murmuration", "swarm", "storm", NULL};
static const char *const PALETTE_NAMES[] = {"theme",  "ember", "ice",    "acid", "matrix", "aurora",
                                            "prism", "potion", "dusk", "ash",  NULL};
static const char *const SHAPE_NAMES[] = {"bird", "arrow", "plane", "dot", NULL};
static const char *const RENDER_NAMES[] = {"kitty", "braille", "sextants", "blocks", NULL};

static const option_t OPTIONS[] = {
    /* short, long, alias, kind, target, min, max, names, metavar, help, group, on -h */
    {'n', "birds", NULL, OPTION_INT, &config.birds, 1, MAX_BIRDS, NULL, "COUNT",
     "how many birds (default 800)", "Flock", 1},
    {'s', "size", NULL, OPTION_INT, &config.bird_size, MIN_BIRD_SIZE, MAX_BIRD_SIZE, NULL, "PIXELS",
     "sprite size in pixels (default 30)", "Flock", 1},
    {'g', "flocks", "groups", OPTION_INT, &config.flocks, 1, MAX_FLOCKS, NULL, "COUNT",
     "flocks that keep to their own kind (default 1)", "Flock", 1},
    {'k', "hawks", NULL, OPTION_INT, &config.hawks, 0, MAX_HAWKS, NULL, "COUNT",
     "predators hunting the flock (default 0)", "Flock", 1},
    {0, "preset", NULL, OPTION_ENUM, &requested_preset, 0, 0, PRESET_NAMES, "NAME",
     "murmuration, swarm, storm", "Flock", 1},
    {0, "seed", NULL, OPTION_INT, &requested_seed, 0, 2147483647, NULL, "N",
     "the same seed gives the same flock", "Flock", 0},

    {0, "boundary", NULL, OPTION_INT, &config.boundary_notch, 0, LEGEND_BAR_CELLS, NULL, "NOTCH",
     "how hard the edges push back (default 4)", "Sliders   0 to 12, as the panel shows them", 0},
    {0, "separation", NULL, OPTION_INT, &config.separation_notch, 0, LEGEND_BAR_CELLS, NULL,
     "NOTCH", "how much a bird keeps its distance (default 4)",
     "Sliders   0 to 12, as the panel shows them", 0},
    {0, "alignment", NULL, OPTION_INT, &config.alignment_notch, 0, LEGEND_BAR_CELLS, NULL, "NOTCH",
     "how much a bird matches its neighbours (default 4)",
     "Sliders   0 to 12, as the panel shows them", 0},
    {0, "turning", NULL, OPTION_INT, &config.turning_notch, 0, LEGEND_BAR_CELLS, NULL, "NOTCH",
     "sharpest turn a frame, 12 is instant (default 8)",
     "Sliders   0 to 12, as the panel shows them", 0},
    {0, "perception", NULL, OPTION_INT, &requested_perception, MIN_VISION_RADIUS, MAX_VISION_RADIUS,
     NULL, "PIXELS", "how far a bird sees, 12 to 60 (default 36)",
     "Sliders   0 to 12, as the panel shows them", 0},
    {0, "speed", NULL, OPTION_INT, &config.pace_notch, 0, LEGEND_BAR_CELLS, NULL, "NOTCH",
     "how fast the flock flies, 0.2x to 2.6x (default 1, 0.4x)",
     "Sliders   0 to 12, as the panel shows them", 0},
    {0, "avoidance", NULL, OPTION_INT, &config.avoid_notch, 0, LEGEND_BAR_CELLS, NULL, "NOTCH",
     "how much flocks keep out of each other's way (default 4)",
     "Sliders   0 to 12, as the panel shows them", 0},

    {'c', "color", "palette", OPTION_ENUM, &config.palette, 0, 0, PALETTE_NAMES, "RAMP",
     "theme, ember, ice, acid, matrix, aurora, prism, potion, dusk, ash", "Look", 1},
    {0, "shape", NULL, OPTION_ENUM, &config.shape, 0, 0, SHAPE_NAMES, "NAME",
     "bird, arrow, plane, dot", "Look", 1},
    {0, "sprite", NULL, OPTION_STRING, &sprite_path, 0, 0, NULL, "FILE",
     "a PNG you supply, kept in its own colours", "Look", 0},
    {'e', "trails", NULL, OPTION_FLAG, &config.trails, 0, 0, NULL, NULL,
     "faint tails behind the flock", "Look", 0},
    {0, "depth", NULL, OPTION_FLAG, &deep_look, 0, 0, NULL, NULL,
     "a second sky further off: smaller, slower, dimmer birds", "Look", 1},
    {'l', "panel", NULL, OPTION_FLAG, &legend_enabled, 0, 0, NULL, NULL,
     "the sliders in the corner from the start; h toggles them", "Look", 1},
    {0, "render", NULL, OPTION_ENUM, &render_mode, 0, 0, RENDER_NAMES, "HOW",
     "braille by default; sextants, blocks, or kitty in Kitty and Ghostty", "Look", 1},

    {0, "matrix", NULL, OPTION_FLAG, &matrix_mode, 0, 0, NULL, NULL, "it is raining birds",
     "Oddities", 0},

    {0, "bench", NULL, OPTION_INT, &bench_frames, 0, 1000000, NULL, "N",
     "run N frames with no terminal, print the numbers, quit", "Output", 0},
    {0, "frames", NULL, OPTION_INT, &frame_limit, 0, 1000000, NULL, "N",
     "quit after N frames, for recording", "Output", 0},
    {0, "snapshot", NULL, OPTION_STRING, &snapshot_path, 0, 0, NULL, "FILE",
     "write the last frame as a PNG", "Output", 0},
    {0, "record", NULL, OPTION_STRING, &record_path, 0, 0, NULL, "FILE",
     "record a GIF, or a .cast for asciinema, with no terminal, and quit", "Output", 0},
    {0, "record-fps", NULL, OPTION_INT, &record_fps, 2, MAX_CAST_FPS, NULL, "RATE",
     "frames a second; a GIF can carry up to 50 (default 25)", "Output", 0},
    {0, "record-seconds", NULL, OPTION_INT, &record_seconds, 1, 120, NULL, "SECONDS",
     "how long the recording runs (default 6)", "Output", 0},
    {0, "record-size", NULL, OPTION_STRING, &requested_record_size, 0, 0, NULL, "COLSxROWS",
     "the size to record at, in cells (default 96x26)", "Output", 0},

    {0, "unlock-fps", NULL, OPTION_FLAG, &unlock_fps, 0, 0, NULL, NULL,
     "render as fast as the terminal allows", "General", 0},
};
enum { OPTION_COUNT = sizeof(OPTIONS) / sizeof(*OPTIONS) };

static const option_example_t EXAMPLES[] = {
    {"cbirds", "a flock in braille, and nothing to read"},
    {"cbirds --preset murmuration", "the starling look"},
    {"cbirds --hawks 2 --color ice", "something to watch"},
    {"cbirds --flocks 3 --color ember", "three of them, keeping to their own"},
    {"cbirds --depth --trails", "a second sky behind the first"},
    {"cbirds --render kitty", "sprites, in Kitty or Ghostty"},
    {"cbirds --record flock.gif", "a GIF, with no terminal in the way"},
    {NULL, NULL},
};
static const char BOIDS_TAGLINE[] = "cbirds — a flock of birds in your terminal.";

static void reset_boids(void) {
    memset(&config, 0, sizeof(config));
    config.birds = 800;
    config.bird_size = 0;
    config.palette = 0;
    config.flocks = 1;
    config.turning_notch = DEFAULT_TURNING_NOTCH;
    config.boundary_notch = DEFAULT_NOTCH;
    config.separation_notch = DEFAULT_NOTCH;
    config.alignment_notch = DEFAULT_NOTCH;
    config.pace_notch = DEFAULT_PACE_NOTCH;
    config.avoid_notch = DEFAULT_NOTCH;
    legend_enabled = 0;
    render_mode = -1; /* RENDER_UNSET */
    deep_look = 0;
    matrix_mode = 0;
    unlock_fps = 0;
    requested_perception = DEFAULT_VISION_RADIUS;
    requested_seed = -1;
    requested_preset = -1;
    frame_limit = 0;
    bench_frames = 0;
    record_fps = 25;
    record_seconds = 6;
    sprite_path = snapshot_path = record_path = requested_record_size = NULL;
}

/* ---- printing ---- */

static void hex(const char *bytes, size_t length) {
    for (size_t i = 0; i < length; i++) printf("%02x", (unsigned char)bytes[i]);
}

static void show_int(const char *name, int value) { printf("%s %d\n", name, value); }

static void show_double(const char *name, double value) {
    uint64_t bits;
    memcpy(&bits, &value, sizeof(bits));
    printf("%s %016llx\n", name, (unsigned long long)bits);
}

static void show_string(const char *name, const char *value) {
    if (value == NULL) {
        printf("%s null\n", name);
        return;
    }
    printf("%s s:", name);
    hex(value, strlen(value));
    putchar('\n');
}

static void dump_boids(void) {
    show_int("birds", config.birds);
    show_int("size", config.bird_size);
    show_int("flocks", config.flocks);
    show_int("hawks", config.hawks);
    show_int("preset", requested_preset);
    show_int("seed", requested_seed);
    show_int("boundary", config.boundary_notch);
    show_int("separation", config.separation_notch);
    show_int("alignment", config.alignment_notch);
    show_int("turning", config.turning_notch);
    show_int("perception", requested_perception);
    show_int("speed", config.pace_notch);
    show_int("avoidance", config.avoid_notch);
    show_int("color", config.palette);
    show_int("shape", config.shape);
    show_string("sprite", sprite_path);
    show_int("trails", config.trails);
    show_int("depth", deep_look);
    show_int("panel", legend_enabled);
    show_int("render", render_mode);
    show_int("matrix", matrix_mode);
    show_int("bench", bench_frames);
    show_int("frames", frame_limit);
    show_string("snapshot", snapshot_path);
    show_string("record", record_path);
    show_int("record-fps", record_fps);
    show_int("record-seconds", record_seconds);
    show_string("record-size", requested_record_size);
    show_int("unlock-fps", unlock_fps);
}

/* ---- tests/options_test.c: TABLE, reset() and the usage EXAMPLES ---- */

static int birds, quiet, mono, palette;
static double weight;
static const char *label;

static const char *const PALETTES[] = {"mono", "flame", "ice", NULL};

static const option_t TABLE[] = {
    {'n', "birds", "boids", OPTION_INT, &birds, 1, 4096, NULL, "COUNT", "how many boids", "Flock",
     1},
    {'w', "weight", NULL, OPTION_DOUBLE, &weight, 0.0, 1.0, NULL, "VALUE", "a weight", "Flock", 0},
    {'q', "quiet", NULL, OPTION_FLAG, &quiet, 0, 0, NULL, NULL, "say less", "Output", 1},
    {'m', "mono", NULL, OPTION_FLAG, &mono, 0, 0, NULL, NULL, "one colour", "Output", 0},
    {'P', "palette", NULL, OPTION_ENUM, &palette, 0, 0, PALETTES, "NAME", "colour scheme", "Output",
     0},
    {0, "label", NULL, OPTION_STRING, &label, 0, 0, NULL, "TEXT", "a caption", "Output", 0},
};
enum { COUNT = sizeof(TABLE) / sizeof(*TABLE) };

static const option_example_t TEST_EXAMPLES[] = {
    {"cbirds -n 1500", "a bigger flock"}, {"cbirds", "the default"}, {NULL, NULL}};

static void reset_test(void) {
    birds = 800;
    weight = 0.5;
    quiet = mono = palette = 0;
    label = NULL;
}

static void dump_test(void) {
    show_int("birds", birds);
    show_double("weight", weight);
    show_int("quiet", quiet);
    show_int("mono", mono);
    show_int("palette", palette);
    show_string("label", label);
}

static int shy;
static const option_t QUOTED[] = {{0, "shy", NULL, OPTION_FLAG, &shy, 0, 0, NULL, NULL,
                                   "keeps out of each other's [way] \"$HOME\"", "Flock", 1}};

static void reset_quoted(void) { shy = 0; }
static void dump_quoted(void) { show_int("shy", shy); }

/* ---- a synthetic table for the corners the real ones do not reach ---- */

static int x_fast, x_panel, x_x, x_no_x, x_int, x_help, x_version, x_aitch, x_vee;
static int x_dup_one, x_dup_two, x_empty, x_choice, x_dash, x_accent, x_utf, x_hundred, x_sixty4;
static int x_int_file, x_blank;
static double x_double;
static const char *x_file, *x_sixty3, *x_eighty, *x_eighty8;

static const char *const EMPTY_NAMES[] = {NULL};
static const char *const CHOICE_NAMES[] = {"alpha", "beta", "", "alpha", "Gamma", NULL};

static const option_t SYNTHETIC[] = {
    /* A flag with an alias, so --no-ALIAS; a metavar a flag never shows. */
    {'f', "fast", "quick", OPTION_FLAG, &x_fast, 0, 0, NULL, "IGNORED", "go fast", "Alpha", 1},
    /* An off switch, named for what it does, with an alias. */
    {'o', "no-panel", "hide", OPTION_OFF, &x_panel, 0, 0, NULL, "IGNORED", "hide the panel",
     "Alpha", 0},
    /* --no-x finds the flag x before the flag no-x. */
    {0, "x", NULL, OPTION_FLAG, &x_x, 0, 0, NULL, NULL, "ex", "Beta", 1},
    {0, "no-x", NULL, OPTION_FLAG, &x_no_x, 0, 0, NULL, NULL, "no ex", "Beta", 0},
    /* A negative range and no metavar; back to a group already shown. */
    {'i', "int", "integer", OPTION_INT, &x_int, -5, 5, NULL, NULL, "an int", "Alpha", 1},
    {'d', "double", NULL, OPTION_DOUBLE, &x_double, -2.5, 1e6, NULL, "REAL", "a double", "Gamma",
     0},
    /* Rows the special switches shadow, except in the forms they do not. */
    {0, "help", NULL, OPTION_INT, &x_help, 0, 9, NULL, "N", "shadowed by --help", "Gamma", 1},
    {0, "version", NULL, OPTION_INT, &x_version, 0, 9, NULL, "N", "shadowed by --version",
     "General", 1},
    {'h', "aitch", NULL, OPTION_FLAG, &x_aitch, 0, 0, NULL, NULL, "never reached as -h", "General",
     0},
    {'V', "vee", NULL, OPTION_FLAG, &x_vee, 0, 0, NULL, NULL, "never reached as -V", "Gamma", 1},
    /* Two rows share a shorthand: the first wins. */
    {'D', "dup-one", NULL, OPTION_FLAG, &x_dup_one, 0, 0, NULL, NULL, "first D", "Gamma", 1},
    {'D', "dup-two", NULL, OPTION_FLAG, &x_dup_two, 0, 0, NULL, NULL, "second D", "Gamma", 1},
    /* An enumeration of nothing, and one with an empty and a repeated name. */
    {'e', "empty", NULL, OPTION_ENUM, &x_empty, 0, 0, EMPTY_NAMES, "NONE", "no choices", "Gamma",
     1},
    {'c', "choice", "pick", OPTION_ENUM, &x_choice, 0, 0, CHOICE_NAMES, "FILE",
     "choose [one] 'of' \"them\" $x \\ back", "D\xc3\xa9"
                                              "lta",
     1},
    {'F', "file", NULL, OPTION_STRING, &x_file, 0, 0, NULL, "FILE", "a file", "D\xc3\xa9"
                                                                              "lta",
     0},
    {0, "int-file", NULL, OPTION_INT, &x_int_file, 0, 100, NULL, "FILE", "a number called FILE",
     "D\xc3\xa9"
     "lta",
     0},
    /* Shorthands only a cluster can reach, or outside ASCII. */
    {'-', "dash", NULL, OPTION_FLAG, &x_dash, 0, 0, NULL, NULL, "a dash", "Epsilon", 1},
    {'\xe9', "accent", NULL, OPTION_FLAG, &x_accent, 0, 0, NULL, NULL, "an \xc3\xa9", "Epsilon",
     1},
    {'u', "utf", NULL, OPTION_INT, &x_utf, 0, 9, NULL, "\xc3\x89"
                                                       "CHELLE",
     "help \xe2\x80\x94 with \xc3\xbcn\xc3\xaf"
     "code",
     "Epsilon", 1},
    /* Lengths either side of what nearest_name measures, and of the 96 byte
     * line render_option builds (the 88 byte name leaves no room for its
     * metavar; a longer one with a metavar would overrun in the C). */
    {0, "sixty-three-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm", NULL, OPTION_STRING,
     &x_sixty3, 0, 0, NULL, "M\xc3\x89TA", "sixty-three bytes", "Long", 0},
    {0, "sixty-four-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-n", NULL, OPTION_FLAG,
     &x_sixty4, 0, 0, NULL, NULL, "sixty-four bytes", "Long", 1},
    {0, "eighty-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-nnn-ooo-ppp-qqq-rrr-s", NULL,
     OPTION_STRING, &x_eighty, 0, 0, NULL, "LONGMETAVAR", "eighty bytes", "Long", 0},
    {0,
     "eighty-eight-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-nnn-ooo-ppp-qqq-rrr-sss",
     NULL, OPTION_STRING, &x_eighty8, 0, 0, NULL, "CUT", "eighty-eight bytes", "Long", 1},
    {0,
     "one-hundred-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-nnn-ooo-ppp-qqq-rrr-sss-ttt-"
     "uuu-vvvw",
     NULL, OPTION_FLAG, &x_hundred, 0, 0, NULL, NULL, "a hundred bytes", "Long", 0},
    /* An empty alias: what "--=" and a bare "--no-" could find. */
    {0, "blank", "", OPTION_FLAG, &x_blank, 0, 0, NULL, NULL, "an empty alias", "Long", 0},
};
enum { SYNTHETIC_COUNT = sizeof(SYNTHETIC) / sizeof(*SYNTHETIC) };

static const option_example_t SYNTHETIC_EXAMPLES[] = {
    {"prog --fast", "quickly"},
    {"prog --choice \xc3\xa9", "an accent, padded by bytes"},
    {"", "nothing at all"},
    {NULL, NULL},
};

static void reset_synthetic(void) {
    x_fast = 0;
    x_panel = 1;
    x_x = x_no_x = 1;
    x_int = 2;
    x_help = x_version = 0;
    x_aitch = x_vee = x_dup_one = x_dup_two = 0;
    x_empty = x_choice = -1;
    x_dash = x_accent = x_utf = x_hundred = x_sixty4 = x_int_file = 0;
    x_blank = 1;
    x_double = 0.5;
    x_file = x_sixty3 = x_eighty = x_eighty8 = NULL;
}

static void dump_synthetic(void) {
    show_int("fast", x_fast);
    show_int("panel", x_panel);
    show_int("x", x_x);
    show_int("no-x", x_no_x);
    show_int("int", x_int);
    show_double("double", x_double);
    show_int("help", x_help);
    show_int("version", x_version);
    show_int("aitch", x_aitch);
    show_int("vee", x_vee);
    show_int("dup-one", x_dup_one);
    show_int("dup-two", x_dup_two);
    show_int("empty", x_empty);
    show_int("choice", x_choice);
    show_string("file", x_file);
    show_int("int-file", x_int_file);
    show_int("dash", x_dash);
    show_int("accent", x_accent);
    show_int("utf", x_utf);
    show_string("sixty-three", x_sixty3);
    show_int("sixty-four", x_sixty4);
    show_string("eighty", x_eighty);
    show_string("eighty-eight", x_eighty8);
    show_int("one-hundred", x_hundred);
    show_int("blank", x_blank);
}

typedef struct {
    const option_t *table;
    size_t count;
    void (*reset)(void);
    void (*dump)(void);
    const option_example_t *examples;
    const char *tagline;
} table_t;

static const table_t TABLES[] = {
    {OPTIONS, OPTION_COUNT, reset_boids, dump_boids, EXAMPLES, BOIDS_TAGLINE},
    {TABLE, COUNT, reset_test, dump_test, TEST_EXAMPLES, "A flock in your terminal."},
    {SYNTHETIC, SYNTHETIC_COUNT, reset_synthetic, dump_synthetic, SYNTHETIC_EXAMPLES,
     "Synthetic \xe2\x80\x94 corners."},
    {QUOTED, 1, reset_quoted, dump_quoted, NULL, NULL},
};

static const option_example_t NO_EXAMPLES[] = {{NULL, NULL}};

/* ---- the command reader ---- */

static void die(const char *why, const char *token) {
    fprintf(stderr, "options_oracle: %s: %s\n", why, token);
    exit(3);
}

static const table_t *table_named(const char *token) {
    if (strcmp(token, "B") == 0) return &TABLES[0];
    if (strcmp(token, "T") == 0) return &TABLES[1];
    if (strcmp(token, "X") == 0) return &TABLES[2];
    if (strcmp(token, "Q") == 0) return &TABLES[3];
    die("no such table", token);
    return NULL;
}

static int nibble(char c, const char *token) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    die("bad hex", token);
    return 0;
}

/* 'x' then hex; a fresh NUL terminated copy that lives to the end. */
static char *bytes(const char *token) {
    size_t length = strlen(token);
    if (token[0] != 'x' || length % 2 != 1) die("bad byte string", token);
    char *out = malloc(length / 2 + 1);
    if (out == NULL) die("out of memory", token);
    for (size_t i = 0; i < length / 2; i++) {
        out[i] = (char)(nibble(token[1 + 2 * i], token) * 16 + nibble(token[2 + 2 * i], token));
        if (out[i] == '\0') die("NUL in a byte string", token);
    }
    out[length / 2] = '\0';
    return out;
}

static size_t size_of(const char *token) {
    char *end;
    unsigned long long value = strtoull(token, &end, 10);
    if (end == token || *end != '\0') die("bad size", token);
    return (size_t)value;
}

static double from_bits(const char *token) {
    char *end;
    uint64_t bits = strtoull(token, &end, 16);
    double value;
    if (end == token || *end != '\0') die("bad bits", token);
    memcpy(&value, &bits, sizeof(value));
    return value;
}

static void parse_and_show(const option_t *table, size_t count, size_t size, int argc,
                           char **argv) {
    char *error = malloc(size + 1);
    if (error == NULL) die("out of memory", "error buffer");
    options_status_t status = options_parse(table, count, argc, argv, error, size);
    printf("status %d %s\n", (int)status, options_status_string(status));
    printf("message ");
    if (size > 0) hex(error, strlen(error));
    putchar('\n');
    free(error);
}

/* argv for options_parse: "cbirds" then the rest of the tokens. */
static char **arguments(char **tokens, int count, int *argc) {
    char **argv = malloc(sizeof(char *) * (size_t)(count + 2));
    if (argv == NULL) die("out of memory", "argv");
    argv[0] = (char *)"cbirds";
    for (int i = 0; i < count; i++) argv[1 + i] = bytes(tokens[i]);
    argv[count + 1] = NULL;
    *argc = count + 1;
    return argv;
}

static void show_captured(FILE *file, const char *field) {
    long length = ftell(file);
    if (length < 0) die("ftell", field);
    rewind(file);
    char *text = malloc((size_t)length + 1);
    if (text == NULL || fread(text, 1, (size_t)length, file) != (size_t)length)
        die("reading back", field);
    printf("%s ", field);
    hex(text, (size_t)length);
    putchar('\n');
    free(text);
    fclose(file);
}

static int r_int;
static double r_double;

static void command(char **tokens, int count) {
    if (count == 0) die("empty command", "");
    const char *verb = tokens[0];
    if (strcmp(verb, "p") == 0) {
        if (count < 3) die("p wants TABLE SIZE", verb);
        const table_t *t = table_named(tokens[1]);
        size_t size = size_of(tokens[2]);
        int argc;
        char **argv = arguments(tokens + 3, count - 3, &argc);
        t->reset();
        parse_and_show(t->table, t->count, size, argc, argv);
        t->dump();
    } else if (strcmp(verb, "r") == 0) {
        if (count < 5) die("r wants KIND MIN MAX SIZE", verb);
        int integer = strcmp(tokens[1], "i") == 0;
        option_t row = {0,    "x",  NULL, integer ? OPTION_INT : OPTION_DOUBLE, NULL, 0, 0, NULL,
                        "N",  "a number", "G", 1};
        row.target = integer ? (void *)&r_int : (void *)&r_double;
        row.minimum = from_bits(tokens[2]);
        row.maximum = from_bits(tokens[3]);
        size_t size = size_of(tokens[4]);
        int argc;
        char **argv = arguments(tokens + 5, count - 5, &argc);
        r_int = 7;
        r_double = 0.5;
        parse_and_show(&row, 1, size, argc, argv);
        if (integer)
            show_int("x", r_int);
        else
            show_double("x", r_double);
    } else if (strcmp(verb, "u") == 0) {
        if (count != 6) die("u wants TABLE EVERYTHING PROGRAM TAGLINE EXAMPLES", verb);
        const table_t *t = table_named(tokens[1]);
        int everything = (int)size_of(tokens[2]);
        const char *program = bytes(tokens[3]);
        const char *tagline = strcmp(tokens[4], "-") == 0   ? NULL
                              : strcmp(tokens[4], "=") == 0 ? t->tagline
                                                            : bytes(tokens[4]);
        const option_example_t *examples = strcmp(tokens[5], "0") == 0   ? NULL
                                           : strcmp(tokens[5], "1") == 0 ? t->examples
                                                                         : NO_EXAMPLES;
        FILE *out = tmpfile();
        if (out == NULL) die("tmpfile", verb);
        options_usage(out, program, tagline, examples, t->table, t->count, everything);
        show_captured(out, "usage");
    } else if (strcmp(verb, "c") == 0) {
        if (count != 4) die("c wants TABLE SHELL PROGRAM", verb);
        const table_t *t = table_named(tokens[1]);
        const char *shell = bytes(tokens[2]);
        const char *program = bytes(tokens[3]);
        FILE *out = tmpfile();
        if (out == NULL) die("tmpfile", verb);
        int known = options_completion(out, shell, program, t->table, t->count);
        printf("return %d\n", known);
        show_captured(out, "output");
    } else {
        die("unknown command", verb);
    }
    printf("end\n");
}

int main(int argc, char **argv) {
    if (argc >= 2 && strcmp(argv[1], "argv") == 0) {
        if (argc < 4) die("argv wants TABLE SIZE", argv[1]);
        const table_t *t = table_named(argv[2]);
        size_t size = size_of(argv[3]);
        /* The arguments as the OS delivered them, behind the program name. */
        char **rest = argv + 3;
        rest[0] = (char *)"cbirds";
        t->reset();
        parse_and_show(t->table, t->count, size, argc - 3, rest);
        t->dump();
        printf("end\n");
        return 0;
    }

    size_t capacity = 1 << 16, length = 0;
    char *input = malloc(capacity);
    if (input == NULL) die("out of memory", "input");
    for (size_t got; (got = fread(input + length, 1, capacity - length, stdin)) > 0;) {
        length += got;
        if (length == capacity) {
            capacity *= 2;
            input = realloc(input, capacity);
            if (input == NULL) die("out of memory", "input");
        }
    }
    input[length] = '\0';

    char **tokens = NULL;
    size_t token_capacity = 0;
    for (char *line = input; *line != '\0';) {
        char *newline = strchr(line, '\n');
        if (newline == NULL) die("unterminated command", line);
        *newline = '\0';
        int count = 0;
        for (char *token = line; token != NULL;) {
            char *space = strchr(token, ' ');
            if (space != NULL) *space = '\0';
            if ((size_t)count + 1 > token_capacity) {
                token_capacity = token_capacity ? token_capacity * 2 : 64;
                tokens = realloc(tokens, sizeof(char *) * token_capacity);
                if (tokens == NULL) die("out of memory", "tokens");
            }
            tokens[count++] = token;
            token = space != NULL ? space + 1 : NULL;
        }
        command(tokens, count);
        line = newline + 1;
    }
    fflush(stdout);
    return 0;
}

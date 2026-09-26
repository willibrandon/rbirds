/*
 * The global state each cbirds boids test starts from, captured from the
 * unmodified suite running in its own order.
 *
 * tests/boids_test.c runs its tests in one process, and several start from
 * what the tests before them left in boids.c's globals: the random state, the
 * panel switch, the hawks, the formation. tests/c_boids.rs starts each
 * translated test from exactly that state, so this program runs the whole
 * C suite, unmodified, and prints the complete state as each test is entered.
 *
 * Each test's name is a variadic macro, which the preprocessor expands
 * differently at its definition, `test_x(void)`, and at its call in main,
 * `test_x()`: the definition is renamed, and the call becomes a dump followed
 * by the renamed function. No reference file is changed.
 *
 * Output: for each test, "entry NAME", the "rbirds-sim-trace 1" dump (no
 * birds: the suite's birds are the tests' own locals), a "settings" line, and
 * "end". The suite's own assertions still run; a failure aborts the program.
 */
#include <stdio.h>

static void rbirds_enter(const char *name);

#define RBIRDS_PICK(name, ...) RBIRDS_PICK_##__VA_ARGS__(name)
#define RBIRDS_PICK_void(name) name##_body(void)
#define RBIRDS_PICK_(name) (rbirds_enter(#name), name##_body())

#define test_a_closed_pipe_leaves_the_terminal_as_it_was(...) RBIRDS_PICK(test_a_closed_pipe_leaves_the_terminal_as_it_was, __VA_ARGS__)
#define test_the_trig_lookup_covers_the_circle(...) RBIRDS_PICK(test_the_trig_lookup_covers_the_circle, __VA_ARGS__)
#define test_the_frame_rate_can_be_unlocked(...) RBIRDS_PICK(test_the_frame_rate_can_be_unlocked, __VA_ARGS__)
#define test_engine_matches_brute_force(...) RBIRDS_PICK(test_engine_matches_brute_force, __VA_ARGS__)
#define test_boundary_bands_follow_the_viewport(...) RBIRDS_PICK(test_boundary_bands_follow_the_viewport, __VA_ARGS__)
#define test_the_edge_pushes_harder_the_further_out_a_bird_is(...) RBIRDS_PICK(test_the_edge_pushes_harder_the_further_out_a_bird_is, __VA_ARGS__)
#define test_bottom_band_scales_on_a_short_viewport(...) RBIRDS_PICK(test_bottom_band_scales_on_a_short_viewport, __VA_ARGS__)
#define test_birds_start_spread_inside_the_free_region(...) RBIRDS_PICK(test_birds_start_spread_inside_the_free_region, __VA_ARGS__)
#define test_a_grown_flock_starts_its_new_birds_clean(...) RBIRDS_PICK(test_a_grown_flock_starts_its_new_birds_clean, __VA_ARGS__)
#define test_the_recording_rate_is_one_a_gif_has(...) RBIRDS_PICK(test_the_recording_rate_is_one_a_gif_has, __VA_ARGS__)
#define test_birds_bank_rather_than_snap(...) RBIRDS_PICK(test_birds_bank_rather_than_snap, __VA_ARGS__)
#define test_the_konami_code(...) RBIRDS_PICK(test_the_konami_code, __VA_ARGS__)
#define test_only_the_rain_has_a_wind(...) RBIRDS_PICK(test_only_the_rain_has_a_wind, __VA_ARGS__)
#define test_autopilot_wanders_and_yields(...) RBIRDS_PICK(test_autopilot_wanders_and_yields, __VA_ARGS__)
#define test_hawks_hunt_and_the_flock_flees(...) RBIRDS_PICK(test_hawks_hunt_and_the_flock_flees, __VA_ARGS__)
#define test_motion_follows_elapsed_time(...) RBIRDS_PICK(test_motion_follows_elapsed_time, __VA_ARGS__)
#define test_more_flocks_are_more_colours(...) RBIRDS_PICK(test_more_flocks_are_more_colours, __VA_ARGS__)
#define test_the_flock_can_be_laid_out_as_text(...) RBIRDS_PICK(test_the_flock_can_be_laid_out_as_text, __VA_ARGS__)
#define test_presets_set_every_notch(...) RBIRDS_PICK(test_presets_set_every_notch, __VA_ARGS__)
#define test_a_notch_survives_the_round_trip(...) RBIRDS_PICK(test_a_notch_survives_the_round_trip, __VA_ARGS__)
#define test_the_pointer_moves_the_flock(...) RBIRDS_PICK(test_the_pointer_moves_the_flock, __VA_ARGS__)
#define test_the_shade_follows_the_heading(...) RBIRDS_PICK(test_the_shade_follows_the_heading, __VA_ARGS__)
#define test_the_hawk_is_never_the_colour_of_the_flock(...) RBIRDS_PICK(test_the_hawk_is_never_the_colour_of_the_flock, __VA_ARGS__)
#define test_the_help_names_every_ramp(...) RBIRDS_PICK(test_the_help_names_every_ramp, __VA_ARGS__)
#define test_no_ramp_fades_into_a_black_terminal(...) RBIRDS_PICK(test_no_ramp_fades_into_a_black_terminal, __VA_ARGS__)
#define test_the_theme_ramp_never_reaches_the_background(...) RBIRDS_PICK(test_the_theme_ramp_never_reaches_the_background, __VA_ARGS__)
#define test_theme_colours_are_parsed(...) RBIRDS_PICK(test_theme_colours_are_parsed, __VA_ARGS__)
#define test_each_flock_flies_at_its_own_pace(...) RBIRDS_PICK(test_each_flock_flies_at_its_own_pace, __VA_ARGS__)
#define test_the_matrix_is_the_only_thing_that_rains(...) RBIRDS_PICK(test_the_matrix_is_the_only_thing_that_rains, __VA_ARGS__)
#define test_the_sprite_catalogue_has_a_place_for_everything(...) RBIRDS_PICK(test_the_sprite_catalogue_has_a_place_for_everything, __VA_ARGS__)
#define test_the_far_layer_is_another_sky(...) RBIRDS_PICK(test_the_far_layer_is_another_sky, __VA_ARGS__)
#define test_a_seed_draws_the_same_numbers_everywhere(...) RBIRDS_PICK(test_a_seed_draws_the_same_numbers_everywhere, __VA_ARGS__)
#define test_wings_beat_and_sometimes_glide(...) RBIRDS_PICK(test_wings_beat_and_sometimes_glide, __VA_ARGS__)
#define test_braille_unless_asked(...) RBIRDS_PICK(test_braille_unless_asked, __VA_ARGS__)
#define test_a_text_terminal_gets_the_flock_in_braille(...) RBIRDS_PICK(test_a_text_terminal_gets_the_flock_in_braille, __VA_ARGS__)
#define test_a_text_renderer_records_its_cells(...) RBIRDS_PICK(test_a_text_renderer_records_its_cells, __VA_ARGS__)
#define test_a_cast_is_the_flock_as_text(...) RBIRDS_PICK(test_a_cast_is_the_flock_as_text, __VA_ARGS__)
#define test_recording_gives_the_whole_frame_to_the_flock(...) RBIRDS_PICK(test_recording_gives_the_whole_frame_to_the_flock, __VA_ARGS__)
#define test_flocks_keep_to_their_own_side_of_the_sky(...) RBIRDS_PICK(test_flocks_keep_to_their_own_side_of_the_sky, __VA_ARGS__)
#define test_flocks_do_not_align_with_each_other(...) RBIRDS_PICK(test_flocks_do_not_align_with_each_other, __VA_ARGS__)
#define test_a_key_ends_the_intro(...) RBIRDS_PICK(test_a_key_ends_the_intro, __VA_ARGS__)
#define test_mouse_reports_are_parsed(...) RBIRDS_PICK(test_mouse_reports_are_parsed, __VA_ARGS__)
#define test_vision_controls(...) RBIRDS_PICK(test_vision_controls, __VA_ARGS__)
#define test_flicker_free_render_queue(...) RBIRDS_PICK(test_flicker_free_render_queue, __VA_ARGS__)
#define test_legend_panel_layout(...) RBIRDS_PICK(test_legend_panel_layout, __VA_ARGS__)
#define test_legend_values_follow_their_notch(...) RBIRDS_PICK(test_legend_values_follow_their_notch, __VA_ARGS__)
#define test_bar_spans_the_whole_travel(...) RBIRDS_PICK(test_bar_spans_the_whole_travel, __VA_ARGS__)
#define test_one_keypress_is_one_cell(...) RBIRDS_PICK(test_one_keypress_is_one_cell, __VA_ARGS__)
#define test_weights_stop_at_their_bounds(...) RBIRDS_PICK(test_weights_stop_at_their_bounds, __VA_ARGS__)
#define test_legend_repels_towards_the_nearer_way_out(...) RBIRDS_PICK(test_legend_repels_towards_the_nearer_way_out, __VA_ARGS__)
#define test_legend_push_overrules_the_flock(...) RBIRDS_PICK(test_legend_push_overrules_the_flock, __VA_ARGS__)
#define test_no_bird_ever_reaches_the_panel(...) RBIRDS_PICK(test_no_bird_ever_reaches_the_panel, __VA_ARGS__)
#define test_birds_start_clear_of_the_panel(...) RBIRDS_PICK(test_birds_start_clear_of_the_panel, __VA_ARGS__)
#define test_frame_carries_the_panel(...) RBIRDS_PICK(test_frame_carries_the_panel, __VA_ARGS__)
#define test_panel_switches_off_cleanly(...) RBIRDS_PICK(test_panel_switches_off_cleanly, __VA_ARGS__)
#define test_no_legend_leaves_the_corner_to_the_flock(...) RBIRDS_PICK(test_no_legend_leaves_the_corner_to_the_flock, __VA_ARGS__)
#define test_the_speed_slider_flies_the_same_path_faster(...) RBIRDS_PICK(test_the_speed_slider_flies_the_same_path_faster, __VA_ARGS__)
#define test_the_default_size_follows_the_renderer(...) RBIRDS_PICK(test_the_default_size_follows_the_renderer, __VA_ARGS__)
#define test_the_speed_is_a_flag(...) RBIRDS_PICK(test_the_speed_is_a_flag, __VA_ARGS__)
#define test_a_hawk_holds_a_chase_for_a_distance(...) RBIRDS_PICK(test_a_hawk_holds_a_chase_for_a_distance, __VA_ARGS__)
#define test_a_fast_flock_is_flown_in_steps(...) RBIRDS_PICK(test_a_fast_flock_is_flown_in_steps, __VA_ARGS__)
#define test_the_avoidance_slider_needs_two_flocks(...) RBIRDS_PICK(test_the_avoidance_slider_needs_two_flocks, __VA_ARGS__)
#define test_the_avoidance_is_a_flag(...) RBIRDS_PICK(test_the_avoidance_is_a_flag, __VA_ARGS__)
#define test_flocks_avoid_each_other_as_much_as_asked(...) RBIRDS_PICK(test_flocks_avoid_each_other_as_much_as_asked, __VA_ARGS__)

#include "tests/boids_test.c"

#include "state_dump.h"

static void rbirds_enter(const char *name) {
    emit("entry %s\n", name);
    dump_state();
    emit("settings frame_limit=%d bench=%d record_fps=%d record_seconds=%d columns=%d rows=%d "
         "matrix=%d unlock=%d perception=%d seed=%d sprite=%d snapshot=%d record=%d\n",
         frame_limit, bench_frames, record_fps, record_seconds, record_columns, record_rows,
         matrix_mode, unlock_fps, requested_perception, requested_seed, sprite_path != NULL,
         snapshot_path != NULL, record_path != NULL);
    emit("end\n");
    fwrite(out_text, 1, out_length, stderr);
    fflush(stderr);
    out_length = 0;
}

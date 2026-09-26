#!/bin/sh
# Same-host performance comparison against the canonical C build
# (docs/PORTING.md §7). For each workload: two warmups, then SAMPLES
# interleaved C/Rust runs of `--bench`, recording the reported frame time, the
# bytes a frame and the process's peak resident memory. Prints medians, spread
# and the budget verdicts; every observation is kept in target/perf/<date>/
# for the report.
#
# Spread is the sample range over its minimum, the measure the 5% rule
# applies to. The interquartile range over the median is printed beside it:
# when the range is wide but the quartiles are tight, one or two samples met
# contention from elsewhere on the host and the run should be repeated when
# it is quieter.
#
#   tools/perf.sh [SAMPLES [WORKLOAD...]]   default 10 samples, every workload
#
# Naming workloads (the first column below, or "startup") repeats only those,
# for a workload whose samples met contention in a full run.
#
# Big-flock mode (docs/DEVIATIONS.md D-006) has no C equivalent, so its
# workloads (named big-*) are measured for Rust alone, on its threads. Where
# cbirds could fly the same flock, the frames must be the bytes of the
# single-thread workload named beside it (with the same frame count, since
# the bytes are an average), and the time is compared with it. These time
# building frames; tools/pacing.py measures how they reach a terminal.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
samples=${1:-10}
[ "$#" -gt 0 ] && shift
only=" $* "
wanted() { [ "$only" = "  " ] || case "$only" in *" $1 "*) true ;; *) false ;; esac; }
out="target/perf/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$out"

cargo build --release --locked --offline --quiet
rust=${CARGO_TARGET_DIR:-target}/release/rbirds
# The reference, with the canonical flags (docs/PORTING.md §2).
r=.reference/cbirds
c="$out/cbirds"
cc -std=c99 -O3 -g $r/boids.c $r/cells.c $r/font.c $r/gif.c $r/kitty_graphics.c \
    $r/options.c $r/png.c $r/spatial_grid.c -o "$c" -lm

peak() {
    # Peak RSS in KiB of one run, and its stdout into $2.
    case "$(uname -s)" in
        Darwin) /usr/bin/time -l "$@" 2> "$out/time.txt" > "$out/stdout.txt"
                awk '/maximum resident set size/ {print int($1 / 1024)}' "$out/time.txt" ;;
        *) /usr/bin/time -v "$@" 2> "$out/time.txt" > "$out/stdout.txt"
           awk -F: '/Maximum resident set size/ {gsub(/ /, "", $2); print $2}' "$out/time.txt" ;;
    esac
}

median() { sort -n | awk '{v[NR] = $1} END {if (NR % 2) print v[(NR + 1) / 2]; else print (v[NR / 2] + v[NR / 2 + 1]) / 2}'; }
spread() { sort -n | awk 'NR == 1 {lo = $1} {hi = $1} END {if (lo > 0) printf "%.1f", (hi - lo) / lo * 100; else print 0}'; }
# Interquartile range over the median, in percent (quartiles by nearest rank).
iqr() { sort -n | awk '{v[NR] = $1} END {q1 = v[int((NR + 3) / 4)]; q3 = v[int((3 * NR + 3) / 4)]; m = (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2; if (m > 0) printf "%.1f", (q3 - q1) / m * 100; else print 0}'; }

verdicts=0
while read -r name args; do
    [ -z "$name" ] && continue
    wanted "$name" || continue
    for side in c rust; do : > "$out/$name.$side.ms"; : > "$out/$name.$side.kb"; : > "$out/$name.$side.bytes"; done
    for i in 0 1; do "$c" $args > /dev/null; "$rust" $args > /dev/null; done
    i=0
    while [ "$i" -lt "$samples" ]; do
        for side in c rust; do
            exe=$c; [ "$side" = rust ] && exe=$rust
            kb=$(peak "$exe" $args)
            awk '/^frame time/ {print $3}' "$out/stdout.txt" >> "$out/$name.$side.ms"
            awk '/^bytes\/frame/ {print $2}' "$out/stdout.txt" >> "$out/$name.$side.bytes"
            echo "$kb" >> "$out/$name.$side.kb"
        done
        i=$((i + 1))
    done
    cm=$(median < "$out/$name.c.ms"); rm_=$(median < "$out/$name.rust.ms")
    ck=$(median < "$out/$name.c.kb"); rk=$(median < "$out/$name.rust.kb")
    cs=$(spread < "$out/$name.c.ms"); rs=$(spread < "$out/$name.rust.ms")
    ci=$(iqr < "$out/$name.c.ms"); ri=$(iqr < "$out/$name.rust.ms")
    # The frames are deterministic, so every sample's bytes a frame agree.
    bytes=$(sort -u "$out/$name.c.bytes" "$out/$name.rust.bytes")
    bytes_ok=PASS
    [ "$(printf '%s\n' "$bytes" | wc -l | tr -d ' ')" = 1 ] || { bytes_ok=FAIL; bytes=differ; }
    ratio=$(awk -v r="$rm_" -v c="$cm" 'BEGIN {printf "%.3f", r / c}')
    time_ok=$(awk -v x="$ratio" 'BEGIN {print (x <= 1.15) ? "PASS" : "FAIL"}')
    memory_ok=$(awk -v r="$rk" -v c="$ck" 'BEGIN {b = c * 1.25; if (c + 8192 > b) b = c + 8192; print (r <= b) ? "PASS" : "FAIL"}')
    noisy=$(awk -v a="$cs" -v b="$rs" 'BEGIN {print (a > 5 || b > 5) ? " (spread over 5%: repeat on a quieter host)" : ""}')
    printf '%-16s C %8.3f ms  Rust %8.3f ms  x%s %s | peak C %6d KiB Rust %6d KiB %s | bytes/frame %s %s | spread C %s%% Rust %s%%, IQR C %s%% Rust %s%%%s\n' \
        "$name" "$cm" "$rm_" "$ratio" "$time_ok" "$ck" "$rk" "$memory_ok" "$bytes" "$bytes_ok" "$cs" "$rs" "$ci" "$ri" "$noisy" | tee -a "$out/summary.txt"
    [ "$time_ok" = PASS ] && [ "$memory_ok" = PASS ] && [ "$bytes_ok" = PASS ] || verdicts=1
done <<EOF
kitty-800 --bench 4000
kitty-4096 --bench 450 -n 4096
braille-800 --bench 450 --render braille
braille-4096 --bench 120 --render braille -n 4096
sextants-800 --bench 450 --render sextants
blocks-800 --bench 550 --render blocks
busy-800 --bench 1250 --speed 12 --hawks 4 --flocks 3 --depth --trails
busy-4096 --bench 140 -n 4096 --speed 12 --hawks 4 --flocks 3 --depth --trails
busy-braille --bench 320 --render braille --speed 12 --hawks 4 --flocks 3 --depth --trails
EOF

# Big-flock mode: Rust alone. "same" names the one-thread workload above
# with the same flock, or "-" past cbirds' 4096 birds.
while read -r name same args; do
    [ -z "$name" ] && continue
    wanted "$name" || continue
    : > "$out/$name.rust.ms"; : > "$out/$name.rust.kb"; : > "$out/$name.rust.bytes"
    for i in 0 1; do "$rust" $args > /dev/null; done
    i=0
    while [ "$i" -lt "$samples" ]; do
        kb=$(peak "$rust" $args)
        awk '/^frame time/ {print $3}' "$out/stdout.txt" >> "$out/$name.rust.ms"
        awk '/^bytes\/frame/ {print $2}' "$out/stdout.txt" >> "$out/$name.rust.bytes"
        echo "$kb" >> "$out/$name.rust.kb"
        i=$((i + 1))
    done
    threads=$(awk '/^threads/ {print $2}' "$out/stdout.txt")
    rm_=$(median < "$out/$name.rust.ms"); rk=$(median < "$out/$name.rust.kb")
    rs=$(spread < "$out/$name.rust.ms"); ri=$(iqr < "$out/$name.rust.ms")
    bytes=$(sort -u "$out/$name.rust.bytes")
    bytes_ok=PASS
    [ "$(printf '%s\n' "$bytes" | wc -l | tr -d ' ')" = 1 ] || { bytes_ok=FAIL; bytes=differ; }
    against=""
    if [ "$same" != - ]; then
        if [ -s "$out/$same.rust.bytes" ]; then
            [ "$(sort -u "$out/$same.rust.bytes")" = "$bytes" ] || bytes_ok=FAIL
            one=$(median < "$out/$same.rust.ms")
            against=$(awk -v b="$rm_" -v o="$one" -v s="$same" 'BEGIN {printf " | %s on 1 thread %.3f ms, x%.2f faster", s, o, o / b}')
        else
            against=" | run $same too to compare"
        fi
    fi
    noisy=$(awk -v a="$rs" 'BEGIN {print (a > 5) ? " (spread over 5%: repeat on a quieter host)" : ""}')
    printf '%-17s Rust %8.3f ms on %s threads | peak %6d KiB | bytes/frame %s %s%s | spread %s%%, IQR %s%%%s\n' \
        "$name" "$rm_" "$threads" "$rk" "$bytes" "$bytes_ok" "$against" "$rs" "$ri" "$noisy" | tee -a "$out/summary.txt"
    [ "$bytes_ok" = PASS ] || verdicts=1
done <<EOF
big-4096          kitty-4096   --bench 450 --big-flock 4096
big-braille-4096  braille-4096 --bench 120 --render braille --big-flock 4096
big-busy-4096     busy-4096    --bench 140 --big-flock 4096 --speed 12 --hawks 4 --flocks 3 --depth --trails
big-braille-16384 -            --bench 200 --render braille --big-flock 16384
big-sixel-16384   -            --bench 200 --render sixel --big-flock 16384
big-65536         -            --bench 120 --big-flock 65536
big-braille-65536 -            --bench 120 --render braille --big-flock 65536
big-sixel-65536   -            --bench 120 --render sixel --big-flock 65536
EOF

# Startup: the whole process for one frame, sprite construction included.
if wanted startup; then
    for side in c rust; do : > "$out/startup.$side.s"; done
    i=0
    while [ "$i" -lt "$samples" ]; do
        for side in c rust; do
            exe=$c; [ "$side" = rust ] && exe=$rust
            start=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
            "$exe" --bench 1 --render braille > /dev/null
            end=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
            awk -v s="$start" -v e="$end" 'BEGIN {printf "%.6f\n", e - s}' >> "$out/startup.$side.s"
        done
        i=$((i + 1))
    done
    cs=$(median < "$out/startup.c.s"); rs=$(median < "$out/startup.rust.s")
    ratio=$(awk -v r="$rs" -v c="$cs" 'BEGIN {printf "%.3f", r / c}')
    ok=$(awk -v x="$ratio" 'BEGIN {print (x <= 1.20) ? "PASS" : "FAIL"}')
    printf '%-16s C %8.4f s   Rust %8.4f s   x%s %s | spread C %s%% Rust %s%%, IQR C %s%% Rust %s%%\n' startup "$cs" "$rs" "$ratio" "$ok" \
        "$(spread < "$out/startup.c.s")" "$(spread < "$out/startup.rust.s")" "$(iqr < "$out/startup.c.s")" "$(iqr < "$out/startup.rust.s")" | tee -a "$out/summary.txt"
    [ "$ok" = PASS ] || verdicts=1
fi
printf 'binary size: C %s bytes, Rust %s bytes\n' "$(wc -c < "$c" | tr -d ' ')" "$(wc -c < "$rust" | tr -d ' ')" | tee -a "$out/summary.txt"
echo "observations kept in $out"
exit "$verdicts"

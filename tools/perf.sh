#!/bin/sh
# Same-host performance comparison against the canonical C build
# (docs/PORTING.md §7). For each workload: two warmups, then SAMPLES
# interleaved C/Rust runs of `--bench`, recording the reported frame time and
# the process's peak resident memory. Prints medians, spread and the budget
# verdicts; every observation is kept in target/perf/<date>/ for the report.
#
#   tools/perf.sh [SAMPLES]      default 10
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
samples=${1:-10}
out="target/perf/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$out"

cargo build --release --locked --offline --quiet
rust=target/release/rbirds
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

verdicts=0
while read -r name args; do
    [ -z "$name" ] && continue
    for side in c rust; do : > "$out/$name.$side.ms"; : > "$out/$name.$side.kb"; done
    for i in 0 1; do "$c" $args > /dev/null; "$rust" $args > /dev/null; done
    i=0
    while [ "$i" -lt "$samples" ]; do
        for side in c rust; do
            exe=$c; [ "$side" = rust ] && exe=$rust
            kb=$(peak "$exe" $args)
            awk '/^frame time/ {print $3}' "$out/stdout.txt" >> "$out/$name.$side.ms"
            echo "$kb" >> "$out/$name.$side.kb"
        done
        i=$((i + 1))
    done
    cm=$(median < "$out/$name.c.ms"); rm_=$(median < "$out/$name.rust.ms")
    ck=$(median < "$out/$name.c.kb"); rk=$(median < "$out/$name.rust.kb")
    cs=$(spread < "$out/$name.c.ms"); rs=$(spread < "$out/$name.rust.ms")
    ratio=$(awk -v r="$rm_" -v c="$cm" 'BEGIN {printf "%.3f", r / c}')
    time_ok=$(awk -v x="$ratio" 'BEGIN {print (x <= 1.15) ? "PASS" : "FAIL"}')
    memory_ok=$(awk -v r="$rk" -v c="$ck" 'BEGIN {b = c * 1.25; if (c + 8192 > b) b = c + 8192; print (r <= b) ? "PASS" : "FAIL"}')
    noisy=$(awk -v a="$cs" -v b="$rs" 'BEGIN {print (a > 5 || b > 5) ? " (spread over 5%: repeat on a quieter host)" : ""}')
    printf '%-16s C %8.3f ms  Rust %8.3f ms  x%s %s | peak C %6d KiB Rust %6d KiB %s | spread C %s%% Rust %s%%%s\n' \
        "$name" "$cm" "$rm_" "$ratio" "$time_ok" "$ck" "$rk" "$memory_ok" "$cs" "$rs" "$noisy" | tee -a "$out/summary.txt"
    [ "$time_ok" = PASS ] && [ "$memory_ok" = PASS ] || verdicts=1
done <<EOF
kitty-800 --bench 300
kitty-4096 --bench 120 -n 4096
braille-800 --bench 300 --render braille
braille-4096 --bench 60 --render braille -n 4096
sextants-800 --bench 300 --render sextants
blocks-800 --bench 300 --render blocks
busy-800 --bench 200 --speed 12 --hawks 4 --flocks 3 --depth --trails
busy-4096 --bench 60 -n 4096 --speed 12 --hawks 4 --flocks 3 --depth --trails
busy-braille --bench 100 --render braille --speed 12 --hawks 4 --flocks 3 --depth --trails
EOF

# Startup: the whole process for one frame, sprite construction included.
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
printf '%-16s C %8.4f s   Rust %8.4f s   x%s %s\n' startup "$cs" "$rs" "$ratio" "$ok" | tee -a "$out/summary.txt"
[ "$ok" = PASS ] || verdicts=1
printf 'binary size: C %s bytes, Rust %s bytes\n' "$(wc -c < "$c" | tr -d ' ')" "$(wc -c < "$rust" | tr -d ' ')" | tee -a "$out/summary.txt"
echo "observations kept in $out"
exit "$verdicts"

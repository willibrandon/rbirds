#!/bin/sh
# Builds the pinned C reference executable for manual lifecycle comparisons,
# exactly as tests/pty_reference.rs does (through tests/support/oracle.rs):
#
#     cc -std=c99 -Wall -Wextra -O3 -g -I<ref> boids.c cells.c font.c gif.c \
#        kitty_graphics.c options.c png.c spatial_grid.c -o <out> -lm
#
# The reference is $RBIRDS_REFERENCE or .reference/cbirds; it is only read —
# no make, no files written there. It must be at the pinned commit with no
# tracked changes. Output: target/oracle/cbirds, or the path given.
#
#     tools/oracle/cbirds_build.sh [OUTPUT]
#     CC=clang tools/oracle/cbirds_build.sh      # another compiler as a control
set -eu

pinned=cc446fc3cb80733371c62676533adcac2fc10002
root=$(cd "$(dirname "$0")/../.." && pwd)
ref=${RBIRDS_REFERENCE:-$root/.reference/cbirds}
out=${1:-$root/target/oracle/cbirds}

if [ ! -f "$ref/boids.c" ]; then
    echo "no reference sources at $ref" >&2
    exit 1
fi
head=$(git -C "$ref" rev-parse HEAD)
if [ "$head" != "$pinned" ]; then
    echo "reference is at $head, not $pinned" >&2
    exit 1
fi
if [ -n "$(git -C "$ref" status --porcelain --untracked-files=no)" ]; then
    echo "reference checkout $ref has tracked changes" >&2
    exit 1
fi

mkdir -p "$(dirname "$out")"
partial="$out.partial.$$"
${CC:-cc} -std=c99 -Wall -Wextra -O3 -g "-I$ref" \
    "$ref/boids.c" "$ref/cells.c" "$ref/font.c" "$ref/gif.c" "$ref/kitty_graphics.c" \
    "$ref/options.c" "$ref/png.c" "$ref/spatial_grid.c" \
    -o "$partial" -lm
mv "$partial" "$out"
echo "$out"

#!/bin/sh
# Lists every source site where the canonical Apple clang build of the pinned
# reference contracts `a * b + c` into llvm.fmuladd, as FILE:LINE:COLUMN.
#
# Clang decides contraction in its front end, independently of -O level, so
# unoptimized IR with debug locations names each site exactly once. The result
# is checked in as docs/evidence/fma-sites.txt; the `fma_sites` test holds the
# Rust sources' `fma: FILE:LINE:COLUMN` tags to it (see src/fp.rs).
#
# usage: tools/oracle/fma-sites.sh [REFERENCE_DIR] > docs/evidence/fma-sites.txt
set -eu
reference=${1:-.reference/cbirds}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/rbirds-fma.XXXXXX")
trap 'rm -rf "$scratch"' EXIT INT TERM
for source in boids.c cells.c font.c gif.c kitty_graphics.c options.c png.c spatial_grid.c; do
    cc -std=c99 -O0 -g -S -emit-llvm "$reference/$source" -o "$scratch/ir.ll"
    awk -v file="$source" '
        /^![0-9]+ = !DILocation\(line: / {
            id = substr($1, 2)
            match($0, /line: [0-9]+/); line = substr($0, RSTART + 6, RLENGTH - 6)
            match($0, /column: [0-9]+/); column = substr($0, RSTART + 8, RLENGTH - 8)
            location[id] = line ":" column
            next
        }
        /call .*@llvm\.fmuladd\.f(32|64)\(/ {
            match($0, /!dbg ![0-9]+/); sites[++count] = substr($0, RSTART + 6, RLENGTH - 6)
        }
        END { for (i = 1; i <= count; i++) print file ":" location[sites[i]] }
    ' "$scratch/ir.ll"
done | sort -t: -k1,1 -k2,2n -k3,3n

#!/bin/sh
# Lists every sin/cos call the canonical Apple clang -O3 build of the pinned
# reference makes, as FILE:LINE:COLUMN FUNCTION, from each object's branch
# relocations and its DWARF line table (innermost inlined location). The
# result is checked in as docs/evidence/trig-sites.txt; it is what licenses
# src/platform/trig.rs to route every sine and cosine through __sincos_stret.
# Darwin only: it needs objdump and dwarfdump from the Xcode tools.
#
# usage: tools/oracle/trig-sites.sh [REFERENCE_DIR]
set -eu
reference=${1:-.reference/cbirds}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/rbirds-trig.XXXXXX")
trap 'rm -rf "$scratch"' EXIT INT TERM
for source in boids.c cells.c font.c gif.c kitty_graphics.c options.c png.c spatial_grid.c; do
    object="$scratch/${source%.c}.o"
    cc -std=c99 -O3 -g -c "$reference/$source" -o "$object"
    objdump -d -r --no-show-raw-insn "$object" |
        awk '/^ *[0-9a-f]+:/ {address = $1}
             /ARM64_RELOC_BRANCH26|X86_64_RELOC_BRANCH/ && /[ \t]_(sin|cos|__sincos_stret)$/ {
                 sub(/:$/, "", address); print address, $NF }' |
        while read -r address callee; do
            where=$(xcrun dwarfdump --lookup="0x$address" "$object" 2>/dev/null |
                sed -n "s/.*Line info: file '[^']*', line \([0-9]*\), column \([0-9]*\).*/\1:\2/p" | head -1)
            echo "$source:$where ${callee#_}"
        done
done | sort -t: -k1,1 -k2,2n -k3,3n

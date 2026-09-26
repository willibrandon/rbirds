#!/bin/sh
# Records each command live in VHS's terminal (ttyd and xterm.js, headless)
# and reports whether the flock ever stood still while flying: the video is
# recorded at 25 frames a second, and a video frame identical to the one
# before it (no pixel changed by 8 or more) is a still. Only the middle of the
# flight counts, from 4 s in until 2 s before q is typed, so the intro and the
# flight out are left out.
#
#   tools/vhs/stalls.sh [-s SECONDS] 'COMMAND' ...
#
# Commands run in target/vhs-stalls/, where ./rbirds is the release build and
# ./cbirds the pinned C reference, for example:
#
#   tools/vhs/stalls.sh './cbirds -n 4096' './rbirds --big-flock 65536'
#
# Needs vhs 0.12.1 or later, ttyd, ffmpeg and a C compiler.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
seconds=14
if [ "${1:-}" = -s ]; then
    seconds=$2
    shift 2
fi
out=target/vhs-stalls
mkdir -p "$out"
cargo build --release --locked --offline --quiet
cp "${CARGO_TARGET_DIR:-target}/release/rbirds" "$out/rbirds"
r=.reference/cbirds
cc -std=c99 -O3 -g $r/boids.c $r/cells.c $r/font.c $r/gif.c $r/kitty_graphics.c \
    $r/options.c $r/png.c $r/spatial_grid.c -o "$out/cbirds" -lm
last=$((seconds - 2))
n=0
for command in "$@"; do
    n=$((n + 1))
    cat > "$out/run$n.tape" <<EOF
Output run$n.mp4
Set Shell "bash"
Set FontSize 12
Set Width 1400
Set Height 800
Set TypingSpeed 0
Set Framerate 25
Hide
Type "PS1='\$ '; clear" Enter
Show
Type "$command" Enter
Sleep ${seconds}s
Type "q"
Sleep 1s
EOF
    (cd "$out" && vhs -q "run$n.tape")
    ffmpeg -loglevel error -i "$out/run$n.mp4" -vf \
        "tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YMAX:file=-" \
        -f null - | awk -v from=4 -v to="$last" -v command="$command" '
        /pts_time:/ { sub(/.*pts_time:/, ""); t = $1 + 0; next }
        /YMAX=/ {
            sub(/.*YMAX=/, "")
            if (t < from || t > to) next
            frames++
            if ($1 + 0 < 8) { still++; run++; if (run > longest) longest = run }
            else run = 0
        }
        END {
            fps = frames / (to - from)
            printf "%-50s %4.1f video fps | still frames %d of %d | longest still %d ms\n",
                command, fps, still, frames, (fps > 0 ? longest * 1000 / fps : 0)
        }'
done

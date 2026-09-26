#!/bin/sh
# Runs tools/vhs/live.tape for the pinned C build and for rbirds, in the same
# headless terminal, and puts each screenshot pair side by side (C left).
# Output in target/vhs/. Needs vhs 0.12.1 or later (0.12.0 writes nothing),
# ttyd, ImageMagick, and a font with the sextant characters (U+1FB00), such
# as Cascadia Code.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
out=target/vhs
rm -rf "$out"
mkdir -p "$out"
cargo build --release --locked --offline --quiet
r=.reference/cbirds
cc -std=c99 -O3 -g $r/boids.c $r/cells.c $r/font.c $r/gif.c $r/kitty_graphics.c \
    $r/options.c $r/png.c $r/spatial_grid.c -o "$out/cbirds" -lm
cp "${CARGO_TARGET_DIR:-target}/release/rbirds" "$out/rbirds"
# Compares the terminal settings with the ones saved before a run. PENDIN
# (0x20000000 in lflag) is kernel state, not a setting: it is set when a
# program returns to canonical mode with input still queued, and cleared by
# the next read, so it is left out.
cat > "$out/settled" <<'EOS'
#!/bin/sh
settings() {
    tr : '\n' | while IFS== read -r key value; do
        [ "$key" = lflag ] && value=$(printf '%x' $((0x$value & ~0x20000000)))
        echo "$key=$value"
    done
}
stty -g | settings > now.stty
settings < "$1" > saved.stty
if cmp -s saved.stty now.stty; then echo terminal restored; else diff saved.stty now.stty; fi
EOS
chmod +x "$out/settled"
for program in cbirds rbirds; do
    sed -e "s|PROGRAM|$program|g" -e "s|OUT|$out|g" tools/vhs/live.tape > "$out/$program.tape"
    vhs -q "$out/$program.tape"
done
for shot in "$out"/cbirds-*.png; do
    name=${shot#"$out"/cbirds-}
    magick "$shot" "$out/rbirds-$name" -bordercolor gray -border 2 +append "$out/pair-$name"
done
ls "$out"/pair-*.png

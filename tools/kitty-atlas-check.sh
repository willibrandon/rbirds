#!/bin/sh
# Optional integration check; requires an installed Kitty (tested with 0.44.0).
# Uses Kitty's actual parser/graphics manager, but does not measure presentation.
set -eu
cd "$(dirname "$0")/.."
kitty_binary=${KITTY_BIN:-kitty}
if ! command -v "$kitty_binary" >/dev/null 2>&1; then
    if [ -x /Applications/kitty.app/Contents/MacOS/kitty ]; then
        kitty_binary=/Applications/kitty.app/Contents/MacOS/kitty
    else
        echo "Set KITTY_BIN to an installed Kitty executable" >&2
        exit 1
    fi
fi
cargo build --release --locked --offline
mkdir -p target/kitty-atlas-check
build_dir=${CARGO_TARGET_DIR:-target}/release
rustc --edition 2024 -L "dependency=$build_dir/deps" \
    --extern "rbirds=$build_dir/librbirds.rlib" \
    tools/oracle/kitty_atlas_scene.rs -o target/kitty-atlas-check/scene
for size in 4 30 64; do
    target/kitty-atlas-check/scene reference "$size" > "target/kitty-atlas-check/scene-before-$size.bin"
    target/kitty-atlas-check/scene atlas "$size" > "target/kitty-atlas-check/scene-after-$size.bin"
done
"$kitty_binary" --version
"$kitty_binary" +runpy 'exec(open("tools/kitty-atlas-check.py").read(), {"__name__": "__main__"})'

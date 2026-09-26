#!/bin/sh
# Creates the isolated, pinned C reference at .reference/cbirds (docs/PORTING.md
# §2) from a sibling cbirds checkout if there is one, otherwise from upstream,
# and verifies it: the commit, a clean tree, and every tracked file's SHA-256
# against docs/reference-manifest.json. Never modifies the source checkout.
set -eu
commit=cc446fc3cb80733371c62676533adcac2fc10002
root=$(cd "$(dirname "$0")/.." && pwd)
target="$root/.reference/cbirds"
if [ ! -d "$target/.git" ]; then
    mkdir -p "$root/.reference"
    if [ -d "$root/../cbirds/.git" ]; then
        git clone --no-hardlinks --no-checkout "$root/../cbirds" "$target"
    else
        git clone --no-checkout https://github.com/clainstone/cbirds.git "$target"
    fi
fi
git -C "$target" checkout -q --detach "$commit"
head=$(git -C "$target" rev-parse HEAD)
[ "$head" = "$commit" ] || { echo "reference is at $head, not $commit" >&2; exit 1; }
if [ -n "$(git -C "$target" status --porcelain --untracked-files=no)" ]; then
    echo "reference has tracked changes" >&2
    exit 1
fi
# Every tracked file hash, as the manifest pins them.
bad=0
sed -n '/"sha256_by_path"/,/}/p' "$root/docs/reference-manifest.json" |
    grep -E '^[[:space:]]*"[^"]+": "[0-9a-f]{64}"' |
    sed -E 's/^[[:space:]]*"([^"]+)": "([0-9a-f]{64})".*/\2  \1/' > "$root/.reference/manifest.sha256"
(cd "$target" && shasum -a 256 -c "$root/.reference/manifest.sha256" > /dev/null) || bad=1
[ "$bad" = 0 ] || { echo "reference files do not match the manifest" >&2; exit 1; }
echo "reference ok: $target at $commit"

#!/bin/sh
# Runs the test suite natively on a Linux target inside Docker: the Rust port
# and, through the oracles, the C reference built with that system's own GCC,
# the canonical Linux control. The platform argument picks the architecture
# (linux/arm64 runs natively on Apple silicon; linux/amd64 is emulated).
#
#   tools/linux.sh linux/arm64 [cargo test arguments]
set -eu
platform=${1:-linux/arm64}
[ $# -gt 0 ] && shift
root=$(cd "$(dirname "$0")/.." && pwd)
image=${RBIRDS_LINUX_IMAGE:-rust:1.96-bookworm}
arch=$(echo "$platform" | cut -d/ -f2)
extra=${*:---release --all-targets --locked --offline}
exec docker run --rm --platform "$platform" \
    -v "$root":/work -w /work \
    -e CARGO_TARGET_DIR="/work/target/docker-$arch" \
    -e CARGO_HOME=/usr/local/cargo \
    "$image" sh -c "
        git config --global --add safe.directory '*' &&
        gcc --version | head -1 && rustc --version &&
        cargo test $extra"

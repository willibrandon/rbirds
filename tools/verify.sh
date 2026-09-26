#!/bin/sh
# One command for every local gate (docs/PORTING.md §3, §4.D). Each gate prints
# PASS, FAIL or UNAVAILABLE; the script exits non-zero if any gate failed, and
# lists the gates that could not run here so a partial run is never read as a
# complete one.
#
#   tools/verify.sh                  native target
#   tools/verify.sh --all-local      also x86_64 macOS under Rosetta and both
#                                    Linux targets in Docker, where available
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 1
failed=""
unavailable=""
all_local=0
[ "${1:-}" = "--all-local" ] && all_local=1

gate() {
    name=$1
    shift
    printf '== %s\n' "$name"
    if "$@"; then
        printf 'PASS %s\n' "$name"
    else
        printf 'FAIL %s\n' "$name"
        failed="$failed
  $name"
    fi
}

skip() {
    printf 'UNAVAILABLE %s: %s\n' "$1" "$2"
    unavailable="$unavailable
  $1 ($2)"
}

toolchain() {
    # The rustup proxy must be the one resolving cargo, so the pin applies.
    rustc_version=$(rustc --version) || return 1
    echo "$rustc_version"
    case "$rustc_version" in
        "rustc 1.96.0 "*) ;;
        *) echo "rustc is not the pinned 1.96.0 (is Homebrew's rustc first on PATH?)"; return 1 ;;
    esac
    cargo --version
}

dependency_graph() {
    # The whole resolved graph, every target and feature: only rbirds itself.
    tree=$(cargo tree --locked --offline --edges all --target all --all-features --prefix none) || return 1
    echo "$tree"
    [ "$(printf '%s\n' "$tree" | grep -c .)" = 1 ] || return 1
    printf '%s\n' "$tree" | grep -q '^rbirds v' || return 1
    # And the manifest declares no dependency tables at all.
    ! grep -Eq '^\[(target\..*\.)?(dev-|build-)?dependencies' Cargo.toml
}

unsafe_confined() {
    # `unsafe` only in src/platform; everywhere else the lint attributes only.
    found=$(grep -rn 'unsafe' src --include='*.rs' | grep -v '^src/platform/' |
        grep -Ev 'forbid\(unsafe_code\)|deny\(unsafe_code\)|allow\(unsafe_code\)' || true)
    [ -z "$found" ] || { echo "$found"; return 1; }
}

linkage() {
    binary=${CARGO_TARGET_DIR:-target}/release/rbirds
    [ -x "$binary" ] || { echo "no release binary at $binary"; return 1; }
    case "$(uname -s)" in
        Darwin) libraries=$(otool -L "$binary" | tail -n +2 | awk '{print $1}') ;;
        Linux) libraries=$(ldd "$binary" 2>&1 | awk '{print $1}') ;;
        *) echo "no linkage check for $(uname -s)"; return 1 ;;
    esac
    echo "$libraries"
    # Nothing found is a failure, not a pass: it means the check did not run.
    [ -n "$libraries" ] || { echo "could not read the linkage of $binary"; return 1; }
    for library in $libraries; do
        case "$library" in
            /usr/lib/libSystem.B.dylib) ;;
            linux-vdso.so.*|libc.so.*|libm.so.*|libgcc_s.so.*|/lib*/ld-linux*|libpthread.so.*|libdl.so.*) ;;
            *) echo "unexpected library $library"; return 1 ;;
        esac
    done
}

inventory_executed() {
    # Every Rust test the inventory maps a C test to is one libtest will run
    # (not merely one that exists): listed by its own test binary, unignored.
    cargo test --release --locked --offline -- --list --format terse 2>&1 |
        awk '/Running tests\// {sub(/.*Running /, ""); sub(/ .*/, ""); file = $0; next}
             /: test$/ {sub(/: test$/, ""); print file "::" $0}' | sort -u > target/listed-tests.txt
    missing=$(tail -n +2 docs/c-test-inventory.csv | cut -d, -f5 | tr ';' '\n' | grep . | sort -u |
        comm -23 - target/listed-tests.txt)
    [ -z "$missing" ] || { echo "mapped but not run: $missing"; return 1; }
    echo "$(tail -n +2 docs/c-test-inventory.csv | wc -l | tr -d ' ') C tests, every mapping run"
}

install_smoke() {
    prefix=$(mktemp -d "${TMPDIR:-/tmp}/rbirds-install.XXXXXX")
    cargo install --path . --locked --offline --root "$prefix" --quiet || return 1
    "$prefix/bin/rbirds" --version || return 1
    cargo uninstall --root "$prefix" rbirds --quiet || return 1
    [ ! -e "$prefix/bin/rbirds" ] || return 1
    rm -rf "$prefix"
}

gate "pinned C reference" tools/reference.sh
gate "toolchain pin" toolchain
gate "formatting" cargo fmt --all -- --check
gate "check" cargo check --all-targets --all-features --locked --offline
gate "clippy" cargo clippy --all-targets --all-features --locked --offline -- -D warnings
gate "tests (debug)" cargo test --all-targets --all-features --locked --offline
gate "tests (release)" cargo test --release --all-targets --all-features --locked --offline
gate "release build" cargo build --release --locked --offline
gate "C test mappings executed" inventory_executed
gate "dependency graph" dependency_graph
gate "unsafe confined to src/platform" unsafe_confined
gate "native linkage" linkage
gate "install and uninstall" install_smoke

if [ "$all_local" = 1 ]; then
    if [ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] && arch -x86_64 /usr/bin/true 2>/dev/null &&
        rustup target list --installed --toolchain 1.96.0 2>/dev/null | grep -q x86_64-apple-darwin; then
        gate "x86_64-apple-darwin tests (Rosetta)" env RBIRDS_ORACLE_CFLAGS="-arch x86_64" \
            cargo test --release --all-targets --locked --offline --target x86_64-apple-darwin
    else
        skip "x86_64-apple-darwin" "needs an arm64 Mac with Rosetta and the x86_64 target"
    fi
    if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
        for platform in linux/arm64 linux/amd64; do
            gate "$platform tests (Docker)" tools/linux.sh "$platform"
        done
    else
        skip "linux targets" "Docker is not available"
    fi
else
    skip "other targets" "run with --all-local, and in native CI for release evidence"
fi
skip "real terminals" "manual: docs/PORTING.md §6 terminal matrix"
skip "performance budgets" "tools/perf.sh on a quiet host"

printf '\n'
if [ -n "$unavailable" ]; then
    printf 'Not run here:%s\n' "$unavailable"
fi
if [ -n "$failed" ]; then
    printf 'FAILED:%s\n' "$failed"
    exit 1
fi
echo "All gates that could run here passed."

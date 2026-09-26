#!/bin/sh
# Fills docs/c-test-inventory.csv's rust_tests, evidence and status columns for
# every C test whose same-named Rust translation exists in tests/c_<suite>.rs.
# Rows stay pending until their translation exists; tests/inventory.rs then
# checks every mapping resolves to a live #[test].
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
csv="$root/docs/c-test-inventory.csv"
awk -F, -v OFS=, -v root="$root" '
function rust_file(c) {
    sub(/^tests\//, "", c); sub(/_test\.c$/, "", c)
    if (c == "kitty_graphics") c = "kitty"
    return "tests/c_" c ".rs"
}
function evidence(c) {
    if (c ~ /boids/) return "tests/sim_oracle.rs;tests/cli_differential.rs;tests/headless_differential.rs"
    if (c ~ /cells/) return "tests/cells_oracle.rs"
    if (c ~ /kitty/) return "tests/kitty_oracle.rs"
    if (c ~ /options/) return "tests/options_oracle.rs;tests/cli_differential.rs"
    if (c ~ /png/) return "tests/png_oracle.rs"
    if (c ~ /gif/) return "tests/gif_oracle.rs"
    if (c ~ /spatial/) return "tests/sim_oracle.rs"
    return ""
}
NR == 1 { print; next }
{
    file = rust_file($1)
    path = root "/" file
    found = 0
    while ((getline line < path) > 0)
        if (line ~ ("^[[:space:]]*fn " $2 "\\(")) found = 1
    close(path)
    if (found) { $5 = file "::" $2; $6 = evidence($1); $7 = "ported" }
    print
}' "$csv" > "$csv.new" && mv "$csv.new" "$csv"
awk -F, 'NR > 1 {n[$7]++} END {for (s in n) print s, n[s]}' "$csv"

# rbirds

A Rust port of [cbirds](https://github.com/clainstone/cbirds) 1.4.0 — a flock of birds in your terminal — that reproduces the reference's behavior bit for bit: the same flock for the same seed, the same frames, the same files, the same messages and exit codes, the same terminal lifecycle.

```sh
cargo build --release
./target/release/rbirds                 # a flock in braille
./target/release/rbirds --hawks 2 --panel
./target/release/rbirds --render kitty  # sprites, in Kitty or Ghostty
./target/release/rbirds --record flock.gif --seed 42
./target/release/rbirds --help
```

The dependency policy is **Rust's standard library and system libraries only**: the package has no dependencies of any kind, and the binary links nothing but the C library (`libSystem` on macOS, glibc on Linux).

## What "the same" means here

The reference is cbirds 1.4.0 at commit [`cc446fc3cb80733371c62676533adcac2fc10002`](https://github.com/clainstone/cbirds/tree/cc446fc3cb80733371c62676533adcac2fc10002), pinned file by file in [the manifest](docs/reference-manifest.json). The port is checked against that reference built with its own canonical flags, never against a description of it:

- **Simulation**: scripted scenarios run through the *unmodified* C code and through the port with identical injected time, keys and window sizes, and every double is compared as its IEEE bits — recordings, live sessions, every slider notch, every recording rate, sixty-plus seconds of autopilot. No tolerance.
- **Output**: seeded GIF recordings and asciinema casts are byte-identical to the reference's; so are the escape sequences of every renderer, the Kitty protocol stream, the PNG and GIF codecs' bytes, the help text and completions.
- **Behavior**: a cross-process corpus compares exit codes and both output streams for the CLI; a scripted pseudoterminal holds the live program to the reference's observed lifecycle (theme queries, raw mode and its exact undoing, every handled signal, blocked and broken output, snapshots).
- **Tests**: all 112 of the reference's own C tests are translated, each starting from the exact state the C suite enters it with.

Two properties of the C *compiler's* output turned out to be part of the reference's arithmetic, and are reproduced deliberately: Apple clang contracts 92 `a*b+c` sites into fused multiply-adds on arm64, and fuses every `sin`/`cos` pair into `__sincos_stret`, which differs from separate calls by an ulp on arm64. See [the design](docs/DESIGN.md) §5.

The intentional differences are the product identity (`rbirds` in help, completions, version and the cast title) and three [proposed deviations](docs/DEVIATIONS.md), all in behavior the C leaves undefined.

## Verifying

```sh
tools/reference.sh        # clone and verify the pinned C reference into .reference/
tools/verify.sh           # every local gate; --all-local adds x86_64 macOS (Rosetta) and Linux (Docker)
tools/perf.sh             # same-host performance against the C build
```

The oracle tests build the reference with the platform's `cc` and fail loudly without it; `RBIRDS_NO_ORACLE=1` turns that into an explicit skip for machines that cannot host it.

## Documents

- [Design](docs/DESIGN.md): architecture, dependency boundaries, numerical behavior, terminal lifecycle.
- [Port process](docs/PORTING.md): phases, required evidence and release gates.
- [Compatibility contract](docs/COMPATIBILITY.md): the behavior preserved and how it is compared.
- [Evidence](docs/evidence/README.md): what was run, where, and with what result.
- [Deviations](docs/DEVIATIONS.md): proposed, pending acceptance.
- [C test inventory](docs/c-test-inventory.csv): all 112 C tests and their Rust translations.

## License

MIT, as cbirds is; see [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). The translated code, the bird artwork and the bitmap font are the upstream author's work.

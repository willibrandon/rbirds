# rbirds

A planned Rust port of [cbirds](https://github.com/clainstone/cbirds), preserving its terminal flock simulation, controls, renderers, and recording features.

The dependency policy is **Rust's standard library and system libraries only**. There will be no third-party Cargo dependencies, including development or build dependencies.

This repository currently contains the design and migration contract. The Rust application and verification harness have not been implemented. No compatibility gate has passed yet.

- [Design](docs/DESIGN.md): architecture, dependency boundaries, numerical behavior, and terminal lifecycle.
- [Port process](docs/PORTING.md): implementation order, required evidence, and release gates.
- [Compatibility contract](docs/COMPATIBILITY.md): behavior to preserve and how to compare it.
- [C test inventory](docs/c-test-inventory.csv): all 112 existing test entry points, awaiting Rust mappings.
- [Reference manifest](docs/reference-manifest.json): pinned source hashes and the local C baseline check.

The reference is cbirds 1.4.0 at commit [`cc446fc3cb80733371c62676533adcac2fc10002`](https://github.com/clainstone/cbirds/tree/cc446fc3cb80733371c62676533adcac2fc10002). Updating upstream is a separate migration decision; the reference must not drift during the port.

The release criterion is demonstrated compatibility on the named platforms and test corpus. Finite tests cannot guarantee identical behavior for every possible execution. The process requires unresolved differences to remain visible and prevents an incomplete port from being labeled compatible.

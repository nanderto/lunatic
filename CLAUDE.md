# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Lunatic is a WebAssembly-based runtime for actor-style server-side applications, inspired by Erlang. It embeds `wasmtime` to run sandboxed WASM modules as lightweight, preemptively-scheduled processes with isolated stacks/heaps and per-process resource permissions. The crate publishes both a `lunatic` binary and a `lunatic-runtime` library.

## Build / test / lint

```bash
cargo build                 # debug build
cargo build --release
cargo test                  # workspace tests
cargo test -p lunatic-process    # single crate
cargo test <name>           # single test by name substring
cargo bench                 # criterion benchmarks (benches/benchmark.rs)
cargo fmt
cargo clippy
```

The `lunatic` binary doubles as a `cargo test` runner for WASM crates: when `CARGO_MANIFEST_DIR` is set and the argument points at `target/wasm32-(wasi|unknown-unknown)/.../deps/*.wasm`, `main.rs` dispatches to `mode::cargo_test` instead of normal execution. Keep that detection (`src/main.rs`) intact when changing argument handling.

Lunatic 0.12 implied `run` as the default subcommand; `is_run_implied()` in `src/main.rs` preserves that by injecting `run` when argv[1] looks like `--bench`, `--dir`, or `*.wasm`.

## Work process

All changes follow a branch-and-PR flow. Never commit directly to `main`.

1. Create a feature branch off `main` with a descriptive prefix: `feat/<topic>`, `fix/<topic>`, `docs/<topic>`, `refactor/<topic>`, etc.
2. Make commits on the branch satisfying the definition-of-done gates below.
3. Push the branch and open a pull request against `main` with `gh pr create`.
4. The PR is the unit of review and merge; further changes go as additional commits to the same branch and are picked up automatically.

If `gh pr create` fails with "Resource not accessible by personal access token," it means `GITHUB_TOKEN` is overriding the keyring auth. Clear it for the call: `$env:GITHUB_TOKEN=""; gh pr create ...` (PowerShell) — the keyring credential has the `repo` scope needed.

## Definition of done (every change)

Every change that adds or modifies functionality must satisfy these gates before it is considered complete — per increment, not only at phase boundaries:

- **Tests cover new code.** Unit tests for libraries, integration tests for host-function changes (with a guest WASM exercising the import), end-to-end tests for framework features. Every behaviour change has at least one test demonstrating it.
- **Full suite passes.** `cargo test` across the workspace, plus per-crate `cargo test -p <crate>` for any crate touched. Existing tests must continue to pass.
- **Lint passes.** `cargo fmt --check` and `cargo clippy --workspace --all-targets` with no new warnings.
- **Host-function surface is asserted.** Any addition, removal, or rename of a host function updates `wat/all_imports.wat`; the import-signature test passes.
- **Documentation is updated in step.** The relevant page in `/docs` is updated as part of the same change — substrate roadmap when a primitive lands, Orleans plan when a framework phase lands, patterns guide when a new pattern becomes idiomatic, gap table on the index page when a row's status changes.

The phase-level "definition of done" sections in `/docs/erlang-substrate.html` and `/docs/orleans-framework.html` describe what each phase produces. The gates above describe what every individual change within a phase must satisfy.

## Host function changes

Adding, removing, or renaming any host function exposed to guest WASM **requires** a matching edit to `wat/all_imports.wat`. That file is asserted against at load time to guarantee the runtime exposes the full import surface guest modules expect. CI / review will reject host-function changes that don't update it.

## Architecture

The repo is a Cargo workspace. `src/` is the binary + thin embedding library; the real runtime is split across `crates/lunatic-*`, each owning one slice of host-side functionality that gets linked into the wasmtime `Linker`.

Top-level pieces:

- `src/main.rs` + `src/mode/` — CLI entrypoint and subcommand dispatch (`run`, `init`, `login`, `node`, `control`, `deploy`, `app`, plus the special `cargo_test` and `execution` modes).
- `src/state.rs` (`DefaultProcessState`) and `src/config.rs` (`DefaultProcessConfig`) — the per-process state/config bundle that aggregates resource tables (processes, TCP streams, timers, etc.) and is what each `*-api` crate hangs its host functions off via traits.
- `src/lib.rs` — re-exports `Environment`, `WasmProcess`, `Process`, `Signal`, `Finished` from `lunatic-process` so embedders can drive the runtime from Rust.

Crate roles:

- `lunatic-process` — core process abstraction, signals/messages, supervision, scheduling glue around wasmtime.
- `lunatic-process-api` — host functions guests use to spawn/cancel/wait on processes.
- `lunatic-messaging-api`, `lunatic-registry-api`, `lunatic-timer-api`, `lunatic-error-api`, `lunatic-trap-api`, `lunatic-version-api` — host-function modules for the corresponding guest features.
- `lunatic-networking-api` — TCP/UDP/TLS host functions.
- `lunatic-wasi-api` — WASI bindings (built on `wasmtime-wasi` / `wasi-common` v8).
- `lunatic-sqlite-api` — embedded SQLite host functions.
- `lunatic-stdout-capture` — captures and routes guest stdout/stderr.
- `lunatic-metrics-api` — metrics host functions; gated behind the `metrics` feature (default on). The `prometheus` feature additionally pulls in `metrics-exporter-prometheus`.
- `lunatic-distributed`, `lunatic-distributed-api`, `lunatic-control`, `lunatic-control-axum` — distributed-node networking, the control-plane server (axum-based), and the host API guests use to talk across nodes. `lunatic-control-submillisecond` is currently excluded from the workspace.
- `lunatic-common-api` — shared helpers used by the api crates.
- `hash-map-id` — small utility for the integer-keyed resource tables that show up in every per-process state.

When adding a new host capability the typical pattern is: add a crate under `crates/lunatic-<thing>-api`, define a state trait + register function, implement that trait on `DefaultProcessState` in `src/state.rs`, register the linker in the appropriate place, and add the import lines to `wat/all_imports.wat`.

## Versioning / changelog

All workspace crates are versioned in lockstep via `[workspace.dependencies]` in the root `Cargo.toml`. The changelog is generated from conventional commits with `git cliff --config ./Cargo.toml --latest --prepend ./CHANGELOG.md`; commit messages should use `feat:`, `fix:`, `docs:`, `refactor:`, `perf:`, `test:`, `chore:` prefixes.

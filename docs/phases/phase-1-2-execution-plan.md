# Phase 1 completion + Phase 2 — execution plan

Working tracker for the autonomous run that finishes Phase 1 (component model /
WASI Preview 2) and lands Phase 2 (TLS surface reconciliation + outbound HTTP).
Not a committed contract; updated as work lands.

## Starting state (verified 2026-06-24)

- On branch `feat/phase-1-1a-dependency-bump`, PR #3 open, **green** (build + test + clippy).
- wasmtime / wasmtime-wasi / wasi-common all at **v45**. `wasm32-wasip1` and
  `wasm32-wasip2` targets installed. `wasmtime-wasi-http@45` fetchable.
- **Phase 1 1a–1e are effectively complete** inside PR #3:
  - 1a dep bump ✓ · 1b fuel API (`set_fuel`/`fuel_async_yield_interval`) ✓
  - 1c ResourceLimiter ✓ · 1d func_wrap audit (all crates build) ✓
  - 1e WASI Preview 1 (ported onto `wasi-common` sync) ✓
- **Missing: 1f** (Preview 2 / component-model linker path) and **1g** (validation + docs).
- **Phase 2:** 2a (14 `tls_*` host fns absent from `wat/all_imports.wat`) is independent
  and ready. 2b–2e (outbound HTTP) depend on 1f.

## Progress

- ✅ **1a–1e** — landed in the dep-bump PR (#3).
- ✅ **1f** — component-model linker path + `can_use_wasi_preview_2` + smoke test (PR #4).
- ✅ **1g** — validation + docs (PR #5).
- ✅ **2a** — TLS surface reconciliation: 14 `tls_*` imports asserted (PR #6).
- ⬜ **2b–2e** — outbound HTTP.

## Git strategy

Stacked branches; each sub-task its own PR so review stays granular. Base of the
stack is the open dep-bump branch (since it is not yet merged to `main`).

```
feat/phase-1-1a-dependency-bump   (PR #3, 1a–1e)        [base]
  └─ feat/phase-1f-wasi-preview2   (PR, 1f)
       └─ feat/phase-1g-validation (PR, 1g)
            └─ feat/phase-2a-tls-surface   (PR, 2a)
                 └─ feat/phase-2-outbound-http (PR, 2b–2e)
```

Definition-of-done gates (build, test, per-crate test, `fmt --check`, `clippy`,
`wat/all_imports.wat` in lockstep, docs updated) apply to **every** PR.

---

## 1f — WASI Preview 2 alongside Preview 1

Goal: reach the component-model linker path; default off; Preview 1 guests unaffected.

1. **State plumbing.** Add `wasi_p2_ctx: wasmtime_wasi::WasiCtx` + `table:
   wasmtime::component::ResourceTable` to `DefaultProcessState`; impl `WasiView`.
   Gate construction so Preview 1 path is unchanged when the flag is off.
2. **Config flag.** `can_use_wasi_preview_2` on `DefaultProcessConfig` +
   `ProcessConfigCtx` (mirrors `can_spawn_processes` etc.), default `false`.
3. **Runtime component path.** Turn `WasmtimeCompiledModule<T>` into an enum
   (`Module` | `Component`) or add a sibling type; detect component vs module via
   `wasmtime::Engine::detect_precompiled`/`Component::new` fallback at compile time.
   Add a `component::Linker<T>` wiring that runs `wasmtime_wasi::p2::add_to_linker_async`.
4. **Smoke test.** Minimal Preview-2 component fixture (built once with
   `--target wasm32-wasip2`, committed under `tests/fixtures/` or `wat/`) instantiated
   through the component linker; assert it reaches a Preview 2 capability.
5. **Docs.** Note in `wat/all_imports.wat` header what is Preview-1-asserted vs the
   Preview 2 component surface.

Risk: invasive to core runtime types. Keep the classic module path byte-for-byte
unchanged; the component path is strictly additive.

## 1g — Validation + docs

- `cargo build --workspace --all-features --release`, `cargo test --workspace`,
  `fmt --check`, `clippy --workspace --all-targets` all green.
- `cargo build --benches` green.
- Update gap table in `docs/index.html` (wasmtime row → v45); mark Phase 1 done in
  `docs/erlang-substrate.html` and `docs/phases/phase-1-wasmtime-upgrade.html`.

## 2a — Reconcile TLS surface (independent)

Add the 14 `tls_*` imports (exact signatures from
`crates/lunatic-networking-api/src/tls_tcp.rs`) to `wat/all_imports.wat`; the existing
`import_filter_signature_matches` test then asserts them. Pure audit; low risk.

`tls_bind, tls_local_addr, tls_accept, tls_connect, drop_tls_listener,
drop_tls_stream, clone_tls_stream, tls_write_vectored, tls_read, set_tls_read_timeout,
set_tls_write_timeout, get_tls_read_timeout, get_tls_write_timeout, tls_flush`

## 2b–2e — Outbound HTTP

- 2b decision: adopt `wasmtime-wasi-http@45` (component-model native; unblocked by 1f).
- 2c: wire `wasmtime_wasi_http::add_only_http_to_linker_async` on the component linker;
  state impls `WasiHttpView`.
- 2d: `can_outbound_http` config flag + optional host allow-list, mirroring fs preopen.
- 2e: integration test — component guest does an HTTPS GET against a local test server;
  deny test — guest without the flag traps. Update `wat`/docs/gap table.

Open question carried from the sketch: module-form guests cannot use the
component-native HTTP surface; outbound HTTP is offered to component guests only this
phase. Documented as a boundary, not a gap.

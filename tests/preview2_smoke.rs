//! Phase 1f smoke test: prove the WASI Preview 2 component-model linker path is
//! reachable. We compile a real Preview 2 component (built with the
//! `wasm32-wasip2` target, committed under `tests/fixtures/`), wire the Preview 2
//! host surface via `WasmtimeRuntime::compile_component`, and instantiate it.
//!
//! Successful instantiation proves the component linker resolved every WASI
//! Preview 2 import the guest declares. The test does no further work — its job
//! is purely to guard that the path keeps building and linking.

use lunatic_process::runtimes::wasmtime::{default_config, WasmtimeRuntime};
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// Minimal component store state. The Preview 2 `WasiCtx` is not `Sync`, so it
/// deliberately lives here rather than in `DefaultProcessState`.
struct ComponentState {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for ComponentState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

#[tokio::test]
async fn preview2_component_path_is_reachable() {
    let runtime = WasmtimeRuntime::new(&default_config()).expect("runtime builds");

    let wasm = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/preview2_smoke.wasm"
    ))
    .expect("preview2 smoke fixture is present");

    let compiled = runtime
        .compile_component::<ComponentState>(wasm.into())
        .expect("component compiles and pre-instantiates against the Preview 2 linker");

    let state = ComponentState {
        wasi: WasiCtxBuilder::new().build(),
        table: ResourceTable::new(),
    };

    runtime
        .instantiate_component(&compiled, state)
        .await
        .expect("component instantiates via the Preview 2 component linker path");
}

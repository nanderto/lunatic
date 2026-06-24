use std::sync::Arc;

use anyhow::Result;
use wasi_common::I32Exit;
use wasmtime::ResourceLimiter;

use crate::{
    config::{ProcessConfig, UNIT_OF_COMPUTE_IN_INSTRUCTIONS},
    state::ProcessState,
    ExecutionResult, ResultValue,
};

use super::RawWasm;

#[derive(Clone)]
pub struct WasmtimeRuntime {
    engine: wasmtime::Engine,
}

impl WasmtimeRuntime {
    pub fn new(config: &wasmtime::Config) -> Result<Self> {
        let engine = wasmtime::Engine::new(config)?;
        Ok(Self { engine })
    }

    /// Compiles a wasm module to machine code and performs type-checking on host functions.
    pub fn compile_module<T>(&self, data: RawWasm) -> Result<WasmtimeCompiledModule<T>>
    where
        T: ProcessState + 'static,
    {
        let module = wasmtime::Module::new(&self.engine, data.as_slice())?;
        let mut linker = wasmtime::Linker::new(&self.engine);
        // Register host functions to linker.
        <T as ProcessState>::register(&mut linker)?;
        let instance_pre = linker.instantiate_pre(&module)?;
        let compiled_module = WasmtimeCompiledModule::new(data, module, instance_pre);
        Ok(compiled_module)
    }

    pub async fn instantiate<T>(
        &self,
        compiled_module: &WasmtimeCompiledModule<T>,
        state: T,
    ) -> Result<WasmtimeInstance<T>>
    where
        T: ProcessState + Send + ResourceLimiter + 'static,
    {
        let max_fuel = state.config().get_max_fuel();
        let mut store = wasmtime::Store::new(&self.engine, state);
        // Set limits of the store
        store.limiter(|state| state);
        // Define maximum fuel and async yield interval
        store.set_fuel(max_fuel.unwrap_or(u64::MAX))?;
        store.fuel_async_yield_interval(Some(UNIT_OF_COMPUTE_IN_INSTRUCTIONS))?;
        // Create instance
        let instance = compiled_module
            .instantiator()
            .instantiate_async(&mut store)
            .await?;
        // Mark state as initialized
        store.data_mut().initialize();
        Ok(WasmtimeInstance { store, instance })
    }
}

pub struct WasmtimeCompiledModule<T> {
    inner: Arc<WasmtimeCompiledModuleInner<T>>,
}

pub struct WasmtimeCompiledModuleInner<T> {
    source: RawWasm,
    module: wasmtime::Module,
    instance_pre: wasmtime::InstancePre<T>,
}

impl<T> WasmtimeCompiledModule<T> {
    pub fn new(
        source: RawWasm,
        module: wasmtime::Module,
        instance_pre: wasmtime::InstancePre<T>,
    ) -> WasmtimeCompiledModule<T> {
        let inner = Arc::new(WasmtimeCompiledModuleInner {
            source,
            module,
            instance_pre,
        });
        Self { inner }
    }

    pub fn exports(&self) -> impl ExactSizeIterator<Item = wasmtime::ExportType<'_>> {
        self.inner.module.exports()
    }

    pub fn source(&self) -> &RawWasm {
        &self.inner.source
    }

    pub fn instantiator(&self) -> &wasmtime::InstancePre<T> {
        &self.inner.instance_pre
    }
}

impl<T> Clone for WasmtimeCompiledModule<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct WasmtimeInstance<T>
where
    T: Send + 'static,
{
    store: wasmtime::Store<T>,
    instance: wasmtime::Instance,
}

impl<T> WasmtimeInstance<T>
where
    T: Send + 'static,
{
    pub async fn call(mut self, function: &str, params: Vec<wasmtime::Val>) -> ExecutionResult<T> {
        let entry = self.instance.get_func(&mut self.store, function);

        if entry.is_none() {
            return ExecutionResult {
                state: self.store.into_data(),
                result: ResultValue::SpawnError(format!("Function '{function}' not found")),
            };
        }

        let result = entry
            .unwrap()
            .call_async(&mut self.store, &params, &mut [])
            .await;

        ExecutionResult {
            state: self.store.into_data(),
            result: match result {
                Ok(()) => ResultValue::Ok,
                Err(err) => {
                    // If the trap is a result of calling `proc_exit(0)`, treat it as an no-error finish.
                    match err.downcast_ref::<I32Exit>() {
                        Some(I32Exit(0)) => ResultValue::Ok,
                        _ => ResultValue::Failed(err.to_string()),
                    }
                }
            },
        }
    }
}

pub fn default_config() -> wasmtime::Config {
    let mut config = wasmtime::Config::new();
    config
        .debug_info(false)
        // The behavior of fuel running out is defined on the Store
        .consume_fuel(true)
        .wasm_reference_types(true)
        .wasm_bulk_memory(true)
        .wasm_multi_value(true)
        .wasm_multi_memory(true)
        .cranelift_opt_level(wasmtime::OptLevel::SpeedAndSize)
        // Allocate resources on demand because we can't predict how many process will exist
        .allocation_strategy(wasmtime::InstanceAllocationStrategy::OnDemand)
        // Enable the component model so WASI Preview 2 component guests can be
        // instantiated through the component linker path (phase 1f). Harmless for
        // classic module guests, which ignore it.
        .wasm_component_model(true)
        // Disable memory relocation (equivalent to static_memory_forced in older wasmtime)
        .memory_may_move(false);
    config
}

// ---------------------------------------------------------------------------
// WASI Preview 2 / component-model path (phase 1f)
//
// This is strictly additive: the classic `Module` + `Linker` path above is
// unchanged. Component guests take this parallel path, which wires the WASI
// Preview 2 host surface onto a `component::Linker` via `wasmtime_wasi::p2`.
// The store state `T` must expose its `WasiCtx`/`ResourceTable` by implementing
// `wasmtime_wasi::WasiView`.
// ---------------------------------------------------------------------------

use wasmtime::component::{
    Component, InstancePre as ComponentInstancePre, Linker as ComponentLinker,
};
use wasmtime_wasi::WasiView;
use wasmtime_wasi_http::p2::WasiHttpView;

impl WasmtimeRuntime {
    /// Compile a WASI Preview 2 component and pre-instantiate it against a
    /// component linker carrying the Preview 2 host surface.
    pub fn compile_component<T>(&self, data: RawWasm) -> Result<WasmtimeCompiledComponent<T>>
    where
        T: WasiView + 'static,
    {
        let component = Component::new(&self.engine, data.as_slice())?;
        let mut linker: ComponentLinker<T> = ComponentLinker::new(&self.engine);
        // Register the WASI Preview 2 host functions on the component linker.
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        let instance_pre = linker.instantiate_pre(&component)?;
        Ok(WasmtimeCompiledComponent::new(
            data,
            component,
            instance_pre,
        ))
    }

    /// Compile a component with the WASI Preview 2 surface **plus** outbound
    /// `wasi:http` (phase 2c). Use this only for processes whose config grants
    /// outbound HTTP (`can_outbound_http`); the plain [`compile_component`]
    /// leaves the `wasi:http` imports unsatisfied, so a component that needs
    /// them fails to instantiate — that unsatisfied-import failure is the deny
    /// path for processes without the permission.
    pub fn compile_component_with_http<T>(
        &self,
        data: RawWasm,
    ) -> Result<WasmtimeCompiledComponent<T>>
    where
        T: WasiView + WasiHttpView + 'static,
    {
        let component = Component::new(&self.engine, data.as_slice())?;
        let mut linker: ComponentLinker<T> = ComponentLinker::new(&self.engine);
        // Preview 2 first (provides wasi:io / clocks that wasi:http builds on),
        // then the outbound HTTP interfaces.
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
        let instance_pre = linker.instantiate_pre(&component)?;
        Ok(WasmtimeCompiledComponent::new(
            data,
            component,
            instance_pre,
        ))
    }

    /// Instantiate a previously compiled component into a fresh store.
    ///
    /// The component path is intentionally decoupled from `ProcessState` /
    /// `ResourceLimiter`: the WASI Preview 2 `WasiCtx` is not `Sync`, so it
    /// cannot live inside `DefaultProcessState` (which must be `Sync`). Any
    /// `WasiView` store state can be instantiated here.
    pub async fn instantiate_component<T>(
        &self,
        compiled_component: &WasmtimeCompiledComponent<T>,
        state: T,
    ) -> Result<WasmtimeComponentInstance<T>>
    where
        T: WasiView + 'static,
    {
        let mut store = wasmtime::Store::new(&self.engine, state);
        // The engine has `consume_fuel` enabled; give the component a full tank
        // and yield periodically so it cannot starve the scheduler.
        store.set_fuel(u64::MAX)?;
        store.fuel_async_yield_interval(Some(UNIT_OF_COMPUTE_IN_INSTRUCTIONS))?;
        let instance = compiled_component
            .instantiator()
            .instantiate_async(&mut store)
            .await?;
        Ok(WasmtimeComponentInstance { store, instance })
    }
}

pub struct WasmtimeCompiledComponent<T: 'static> {
    inner: Arc<WasmtimeCompiledComponentInner<T>>,
}

struct WasmtimeCompiledComponentInner<T: 'static> {
    source: RawWasm,
    component: Component,
    instance_pre: ComponentInstancePre<T>,
}

impl<T: 'static> WasmtimeCompiledComponent<T> {
    fn new(
        source: RawWasm,
        component: Component,
        instance_pre: ComponentInstancePre<T>,
    ) -> WasmtimeCompiledComponent<T> {
        let inner = Arc::new(WasmtimeCompiledComponentInner {
            source,
            component,
            instance_pre,
        });
        Self { inner }
    }

    pub fn source(&self) -> &RawWasm {
        &self.inner.source
    }

    pub fn component(&self) -> &Component {
        &self.inner.component
    }

    pub fn instantiator(&self) -> &ComponentInstancePre<T> {
        &self.inner.instance_pre
    }
}

impl<T: 'static> Clone for WasmtimeCompiledComponent<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct WasmtimeComponentInstance<T>
where
    T: Send + 'static,
{
    store: wasmtime::Store<T>,
    instance: wasmtime::component::Instance,
}

impl<T> WasmtimeComponentInstance<T>
where
    T: Send + 'static,
{
    /// Access the underlying component instance and store, e.g. to look up and
    /// call a typed export. Used by the Preview 2 smoke test to prove the path
    /// is reachable.
    pub fn store_and_instance(
        &mut self,
    ) -> (&mut wasmtime::Store<T>, &wasmtime::component::Instance) {
        (&mut self.store, &self.instance)
    }

    /// Consume the instance and return the inner store state.
    pub fn into_data(self) -> T {
        self.store.into_data()
    }
}

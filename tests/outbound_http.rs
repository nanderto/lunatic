//! Phase 2e integration tests for outbound `wasi:http`.
//!
//! * `outbound_http_get_reaches_local_server` — the end-to-end happy path: a
//!   component built against `wasi:http/outgoing-handler` performs a real `GET`
//!   against a local TCP server, gated through `compile_component_with_http`.
//!   We assert both that the server observed exactly one request and that the
//!   guest validated the `200 lunatic-http-ok` response (it traps otherwise).
//! * `outbound_http_denied_without_http_surface` — the deny path: the same
//!   component compiled via the plain `compile_component` (no HTTP wired, i.e.
//!   `can_outbound_http == false`) fails to instantiate because its
//!   `wasi:http` imports are unsatisfied.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lunatic_process::runtimes::wasmtime::{default_config, WasmtimeRuntime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::p2::bindings::Command;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::p2::{WasiHttpCtxView, WasiHttpView};
use wasmtime_wasi_http::WasiHttpCtx;

/// Component store state carrying both the WASI Preview 2 context and the
/// `wasi:http` context. Like the Preview 2 smoke test, this lives outside
/// `DefaultProcessState` because the Preview 2 `WasiCtx` is not `Sync`.
struct HttpComponentState {
    wasi: WasiCtx,
    http: WasiHttpCtx,
    table: ResourceTable,
}

impl WasiView for HttpComponentState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for HttpComponentState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: Default::default(),
        }
    }
}

const HTTP_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/outbound_http.wasm"
);

/// Spawn a one-shot local HTTP/1.1 server that answers `200 lunatic-http-ok`
/// and records how many requests it served. Returns the bound authority.
async fn spawn_local_http_server(hits: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let authority = listener.local_addr().unwrap().to_string();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // Read the request headers (GET has no body).
            let mut buf = [0u8; 1024];
            let mut total = Vec::new();
            loop {
                match socket.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        total.extend_from_slice(&buf[..n]);
                        if total.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            hits.fetch_add(1, Ordering::SeqCst);
            let body = b"lunatic-http-ok";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.write_all(body).await;
            let _ = socket.flush().await;
        }
    });

    authority
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outbound_http_get_reaches_local_server() {
    let hits = Arc::new(AtomicUsize::new(0));
    let authority = spawn_local_http_server(hits.clone()).await;

    let runtime = WasmtimeRuntime::new(&default_config()).unwrap();
    let wasm = std::fs::read(HTTP_FIXTURE).expect("outbound-http fixture present");
    let compiled = runtime
        .compile_component_with_http::<HttpComponentState>(wasm.into())
        .expect("component compiles with the wasi:http surface");

    let state = HttpComponentState {
        wasi: WasiCtxBuilder::new()
            .env("HTTP_TARGET_AUTHORITY", &authority)
            .inherit_stdout()
            .build(),
        http: WasiHttpCtx::new(),
        table: ResourceTable::new(),
    };

    let mut instance = runtime
        .instantiate_component(&compiled, state)
        .await
        .expect("component instantiates with HTTP wired");

    let (store, raw) = instance.store_and_instance();
    let command = Command::new(&mut *store, raw).expect("guest is a wasi:cli command");
    let run_result = command
        .wasi_cli_run()
        .call_run(&mut *store)
        .await
        .expect("run invocation does not trap");
    assert!(run_result.is_ok(), "guest run() returned an error exit");

    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "local server should have served exactly one request"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outbound_http_denied_without_http_surface() {
    let runtime = WasmtimeRuntime::new(&default_config()).unwrap();
    let wasm = std::fs::read(HTTP_FIXTURE).expect("outbound-http fixture present");

    // No HTTP wired (models `can_outbound_http == false`): the component's
    // `wasi:http` imports are unsatisfied, so pre-instantiation must fail.
    let denied = runtime.compile_component::<HttpComponentState>(wasm.into());
    assert!(
        denied.is_err(),
        "component requiring wasi:http must fail to instantiate when HTTP is withheld"
    );
}

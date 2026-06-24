//! Minimal WASI Preview 2 component guest used by the phase-1f smoke test.
//!
//! Touching the clock and stdout exercises real Preview 2 capabilities
//! (`wasi:clocks`, `wasi:cli` stdout), which proves the host's component
//! linker wired the Preview 2 surface. The host test only needs to
//! instantiate this component successfully.
fn main() {
    let _ = std::time::SystemTime::now();
    println!("preview2 smoke guest reached");
}

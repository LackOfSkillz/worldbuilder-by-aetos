//! Throwaway spike. Nothing in this crate is the engine.

pub mod corpus;
pub mod detmath;
pub mod probe;

/// Exported to the WASM host. Returns the probe value for one corpus index.
#[no_mangle]
pub extern "C" fn probe_at(index: u64) -> f64 {
    let input = corpus::input_at(index);
    probe::evaluate(&input)
}

#[cfg(not(target_arch = "wasm32"))]
pub mod beast;

#[cfg(feature = "sdr")]
pub mod iqread;

#[cfg(feature = "sero")]
pub mod sero;

#[cfg(feature = "ssh")]
pub mod ssh;

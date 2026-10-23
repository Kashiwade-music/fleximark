//! Role: Expose the sandbox fixture hooks and generated component registration.
pub mod hooks;
#[cfg(target_arch = "wasm32")]
mod registration;
#[cfg(test)]
mod tests;

//! Role: Deliver LSP and RPC requests and session events over the daemon transport.
mod operations;
mod stdio;

pub use stdio::run;

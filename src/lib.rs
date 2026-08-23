pub mod args;
pub mod log;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "web")]
pub mod api;
pub mod core;

#[cfg(feature = "web")]
pub use api::{create_router, AppState};
pub use args::Args;
pub use core::{
    Backend, Config, IpPool, LoadBalancer,
    ServiceConfig, ServiceState,
    init_global_limiter, run_forward,
};
pub use log::{get_log_buffer, push_log, reset_start_time, LogBuffer, LogEntry};
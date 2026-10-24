//! Independent Driver configuration and authenticated Unix socket control with service-enforced inventory access.

mod client;
mod contract;
mod latency;
mod manager;
pub use latency::{
    EngineLatencyLease, LATENCY_ROUTE, LatencyCommand, LatencyReply, RuntimeLatency,
};
mod server;
mod store;
mod transport_compatibility;
pub use transport_compatibility::parse_transport_compatibility;

#[cfg(test)]
mod test_backend;

pub use client::DriverClient;
pub use contract::{
    COMMAND_ROUTE, DriverBackend, DriverCommand, DriverDeviceConfig, DriverError, DriverSnapshot,
    DriverState, NativeDirection, SNAPSHOT_ROUTE,
};
pub use manager::DriverManager;
pub use server::{AccessRequest, PAID_ACCESS_HEADER, PaidAccess, router, router_with_access};

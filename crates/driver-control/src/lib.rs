//! Independent Driver configuration and loopback control with service-enforced inventory access.

mod client;
mod contract;
mod manager;
mod server;
mod store;

#[cfg(test)]
mod test_backend;

pub use client::DriverClient;
pub use contract::{
    COMMAND_ROUTE, DRIVER_PORT, DriverBackend, DriverCommand, DriverDeviceConfig, DriverError,
    DriverSnapshot, DriverState, NativeDirection, SNAPSHOT_ROUTE,
};
pub use manager::DriverManager;
pub use server::{router, router_with_access};

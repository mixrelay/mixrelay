//! Current shared source registry contract; the Rust registry owns every map.
pub(super) use tonenari_macos_transport::source_registry::{
    SourceRegistry, tn_engine_source_registry_clock, tn_engine_source_registry_read_clocked,
    tn_engine_source_registry_set_clock_master,
};

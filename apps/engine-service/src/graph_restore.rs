//! Persisted bare PIDs cannot identify an application across engine lifetimes.

use super::{GraphDocument, NodeConfig};
use tonenari_audio_io::ApplicationSelector;

pub(super) fn require_pid_reselection(document: &mut GraphDocument) {
    for node in &mut document.nodes {
        if let NodeConfig::ApplicationCapture(request) = &mut node.config
            && matches!(request.selector, ApplicationSelector::ProcessId { .. })
        {
            request.requires_reselection = true;
        }
    }
}

#[cfg(test)]
#[path = "graph_restore_tests.rs"]
mod tests;

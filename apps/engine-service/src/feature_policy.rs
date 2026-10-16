//! Admission policy for authored app work, never the Driver's native channel contract.
use crate::{EngineGraphController, graph_catalog::Catalog};
use mixrelay_audio_graph::{
    CompileContext, GraphCompiler, GraphDocument, NodeConfig, TO_OS_PORT_ID,
};
use mixrelay_control_protocol::{ErrorCode, ProtocolError};
use std::collections::BTreeSet;

pub(crate) const FREE_RECORDING_SECONDS: u64 = 2 * 60 * 60;

pub(crate) fn limit(resource: &str, maximum: usize, used: usize) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::FeatureLimit,
        parameters: serde_json::json!({"resource":resource, "limit":maximum, "used":used}),
    }
}

#[derive(Default)]
struct Usage {
    graphs: usize,
    inputs: usize,
    outputs: BTreeSet<String>,
    devices: BTreeSet<String>,
    mappings: usize,
    plugins: usize,
    recorders: usize,
    recording_channels: usize,
    aggregate_capture: bool,
}

impl Usage {
    fn count_export_routes(
        &mut self,
        graph: &GraphDocument,
        compiler: &GraphCompiler,
        context: CompileContext,
        settings: Option<&mixrelay_audio_io::VirtualEndpointSettings>,
    ) {
        let mut aliases = BTreeSet::new();
        for node in &graph.nodes {
            let NodeConfig::AudioDeviceOutput(config) = &node.config else {
                continue;
            };
            let mixrelay_audio_io::DeviceSelector::PersistentUid { uid } = &config.selector else {
                continue;
            };
            let Some(device) = settings.and_then(|settings| {
                settings
                    .devices
                    .iter()
                    .find(|device| device.native_device_ids.contains(uid))
            }) else {
                continue;
            };
            self.outputs.remove(&format!(
                "hardware:{}",
                serde_json::to_string(&config.selector).expect("device selector serialization")
            ));
            self.outputs
                .insert(format!("driver:{}", device.spec.id.as_str()));
            self.devices.insert(device.spec.id.as_str().to_owned());
            aliases.insert(&node.id);
        }
        for edge in &graph.edges {
            let driver = aliases.contains(&edge.destination.node_id)
                || (edge.destination.port_id.as_str() == TO_OS_PORT_ID
                    && graph.nodes.iter().any(|node| {
                        node.id == edge.destination.node_id
                            && matches!(node.config, NodeConfig::VirtualEndpoint(_))
                    }));
            if !driver {
                continue;
            }
            self.mappings += match &edge.channel_mapping {
                mixrelay_audio_graph::ChannelMapping::Explicit { routes } => routes.len(),
                mixrelay_audio_graph::ChannelMapping::Auto => {
                    // Export preserves unavailable work. Resolve only this edge's saved
                    // geometry; an unrelated missing provider cannot block file saving.
                    let nodes = graph
                        .nodes
                        .iter()
                        .filter(|node| {
                            node.id == edge.source.node_id || node.id == edge.destination.node_id
                        })
                        .cloned()
                        .collect();
                    let isolated = GraphDocument::new(graph.revision, nodes, vec![edge.clone()]);
                    compiler.compile(&isolated, context).map_or(0, |plan| {
                        plan.edges().iter().map(|edge| edge.routes().len()).sum()
                    })
                }
            };
        }
    }

    fn nodes(documents: &[&GraphDocument]) -> Self {
        let mut usage = Self {
            graphs: documents.len(),
            ..Self::default()
        };
        for graph in documents {
            usage.count_nodes(graph);
        }
        usage
    }

    fn count_nodes(&mut self, graph: &GraphDocument) {
        let usage = self;
        for node in &graph.nodes {
            match &node.config {
                NodeConfig::ApplicationCapture(request) => {
                    usage.inputs += 1;
                    usage.aggregate_capture |= matches!(
                        request.selector,
                        mixrelay_audio_io::ApplicationSelector::AllExcept { .. }
                    );
                }
                NodeConfig::AudioDeviceInput(_) => usage.inputs += 1,
                NodeConfig::AudioDeviceOutput(config) => {
                    usage.outputs.insert(format!(
                        "hardware:{}",
                        serde_json::to_string(&config.selector)
                            .expect("device selector serialization")
                    ));
                }
                NodeConfig::VirtualEndpoint(config) => {
                    usage.devices.insert(config.id.as_str().to_owned());
                    if config.reference.capture {
                        usage
                            .outputs
                            .insert(format!("driver:{}", config.id.as_str()));
                    }
                }
                NodeConfig::Provider(_) => usage.plugins += 1,
                NodeConfig::Recorder { channels } => {
                    usage.recorders += 1;
                    usage.recording_channels = usage.recording_channels.max(usize::from(*channels));
                }
                _ => {}
            }
        }
    }

    fn measure(
        documents: &[&GraphDocument],
        compiler: &GraphCompiler,
        context: CompileContext,
    ) -> Result<Self, ProtocolError> {
        let mut usage = Self::nodes(documents);
        for graph in documents {
            let plan = compiler
                .compile(graph, context)
                .map_err(|error| crate::graph_catalog::invalid(&error.to_string()))?;
            usage.mappings += plan
                .edges()
                .iter()
                .filter(|edge| {
                    edge.destination().port_id.as_str() == TO_OS_PORT_ID
                        && graph.nodes.iter().any(|node| {
                            node.id == edge.destination().node_id
                                && matches!(node.config, NodeConfig::VirtualEndpoint(_))
                        })
                })
                .map(|edge| edge.routes().len())
                .sum::<usize>();
        }
        Ok(usage)
    }

    // Raw hardware selectors cannot turn Driver destinations into unmetered outputs.
    fn count_driver_aliases(
        &mut self,
        documents: &[&GraphDocument],
        compiler: &GraphCompiler,
        context: CompileContext,
        settings: &mixrelay_audio_io::VirtualEndpointSettings,
    ) -> Result<(), ProtocolError> {
        for graph in documents {
            let mut aliases = BTreeSet::new();
            for node in &graph.nodes {
                let NodeConfig::AudioDeviceOutput(config) = &node.config else {
                    continue;
                };
                let mixrelay_audio_io::DeviceSelector::PersistentUid { uid } = &config.selector
                else {
                    continue;
                };
                let Some(device) = settings
                    .devices
                    .iter()
                    .find(|device| device.native_device_ids.contains(uid))
                else {
                    continue;
                };
                self.outputs.remove(&format!(
                    "hardware:{}",
                    serde_json::to_string(&config.selector).expect("device selector serialization")
                ));
                self.outputs
                    .insert(format!("driver:{}", device.spec.id.as_str()));
                self.devices.insert(device.spec.id.as_str().to_owned());
                aliases.insert(&node.id);
            }
            if aliases.is_empty() {
                continue;
            }
            let plan = compiler
                .compile(graph, context)
                .map_err(|error| crate::graph_catalog::invalid(&error.to_string()))?;
            self.mappings += plan
                .edges()
                .iter()
                .filter(|edge| aliases.contains(&edge.destination().node_id))
                .map(|edge| edge.routes().len())
                .sum::<usize>();
        }
        Ok(())
    }
    fn compare(&self, before: &Self) -> Result<(), ProtocolError> {
        for (resource, cap, proposed, current) in [
            ("graphs", 1, self.graphs, before.graphs),
            (
                "allApplicationsCapture",
                0,
                usize::from(self.aggregate_capture),
                usize::from(before.aggregate_capture),
            ),
            ("inputNodes", 2, self.inputs, before.inputs),
            ("outputDevices", 2, self.outputs.len(), before.outputs.len()),
            (
                "virtualDevices",
                1,
                self.devices.len(),
                before.devices.len(),
            ),
            ("driverMappings", 4, self.mappings, before.mappings),
            ("externalPlugins", 2, self.plugins, before.plugins),
            ("recorders", 1, self.recorders, before.recorders),
            (
                "recordingChannels",
                2,
                self.recording_channels,
                before.recording_channels,
            ),
        ] {
            // Downgrade never rewrites a running paid graph. Edits may reduce its
            // usage or adjust existing controls, but cannot expand an excess.
            if proposed > cap && proposed > current {
                return Err(limit(resource, cap, proposed));
            }
        }
        Ok(())
    }
}

impl EngineGraphController {
    pub(crate) fn is_paid(&self) -> bool {
        (self.paid_access)()
    }

    /// Saving preserves authored work; only admission requires available resources.
    pub(super) fn validate_workspace_export_usage(
        &self,
        catalog: &Catalog,
    ) -> Result<(), ProtocolError> {
        if self.is_paid() {
            return Ok(());
        }
        let settings = self.endpoint_settings();
        let mut documents: Vec<_> = catalog
            .documents()
            .map(|(_, graph)| graph.clone())
            .collect();
        for graph in &mut documents {
            for node in &mut graph.nodes {
                if let NodeConfig::VirtualEndpoint(config) = &mut node.config
                    && let Some(actual) = settings.as_ref().and_then(|settings| {
                        settings
                            .devices
                            .iter()
                            .find(|device| device.spec.id == config.id)
                    })
                {
                    config.spec.clone_from(&actual.spec);
                }
            }
        }
        let references: Vec<_> = documents.iter().collect();
        let mut usage = Usage::nodes(&references);
        let compiler = self.factory.compiler();
        let context = *self.context.read();
        for graph in &documents {
            usage.count_export_routes(graph, &compiler, context, settings.as_ref());
        }
        usage.compare(&Usage::default())
    }

    /// Files and startup recall are new admissions, not reductions of a running paid graph.
    pub(super) fn validate_workspace_feature_usage(
        &self,
        catalog: &Catalog,
    ) -> Result<(), ProtocolError> {
        if self.is_paid() {
            return Ok(());
        }
        let mut documents: Vec<_> = catalog
            .documents()
            .map(|(id, graph)| (id.to_owned(), graph.clone()))
            .collect();
        let authored: Vec<_> = documents.iter().map(|(_, graph)| graph).collect();
        let mut authored_usage = Usage::nodes(&authored);
        // Different selectors can name one output. Count outputs after identity resolution.
        authored_usage.outputs.clear();
        authored_usage.compare(&Usage::default())?;
        self.resolve_execution_devices(&mut documents)?;
        self.resolve_output_identities(&mut documents)?;
        let resolved: Vec<_> = documents.iter().map(|(_, graph)| graph).collect();
        let compiler = self.factory.compiler();
        let context = *self.context.read();
        let mut usage = Usage::measure(&resolved, &compiler, context)?;
        if let Some(settings) = self.endpoint_settings() {
            usage.count_driver_aliases(&resolved, &compiler, context, &settings)?;
        }
        usage.compare(&Usage::default())
    }

    fn resolve_output_identities(
        &self,
        documents: &mut [(String, GraphDocument)],
    ) -> Result<(), ProtocolError> {
        use mixrelay_audio_io::{DeviceDirection, DeviceSelector};
        let selectors: Vec<_> = documents
            .iter()
            .flat_map(|(_, graph)| &graph.nodes)
            .filter_map(|node| match &node.config {
                NodeConfig::AudioDeviceOutput(config) => Some(&config.selector),
                _ => None,
            })
            .collect();
        if selectors.is_empty() {
            return Ok(());
        }
        let devices = self
            .graph_resources
            .device_inventory(&selectors)
            .map_err(|error| crate::graph_catalog::invalid(&error.to_string()))?;
        for (_, graph) in documents {
            for node in &mut graph.nodes {
                if let NodeConfig::AudioDeviceOutput(config) = &mut node.config {
                    let actual = config
                        .selector
                        .resolve(&devices, DeviceDirection::Output, config.input_channels)
                        .map_err(|error| crate::graph_catalog::invalid(&error.to_string()))?;
                    config.selector = DeviceSelector::PersistentUid {
                        uid: actual.uid.clone(),
                    };
                }
            }
        }
        Ok(())
    }

    pub(super) fn validate_feature_usage(
        &self,
        document: &GraphDocument,
        catalog: Option<&Catalog>,
    ) -> Result<(), ProtocolError> {
        if self.is_paid() {
            return Ok(());
        }
        let compiler = self.factory.compiler();
        let context = *self.context.read();
        let mut resolved = catalog.map_or_else(
            || vec![(self.view_graph_id(), document.clone())],
            |value| {
                value
                    .documents()
                    .map(|(id, graph)| (id.to_owned(), graph.clone()))
                    .collect()
            },
        );
        // Execution replaces stale reference widths with Driver-owned widths.
        // Admission must count the same Auto routes without mutating the Driver.
        self.resolve_execution_devices(&mut resolved)?;
        self.resolve_output_identities(&mut resolved)?;
        let proposed: Vec<_> = resolved.iter().map(|(_, graph)| graph).collect();
        let current = self.graph.read().clone();
        let previous = self.catalog_candidate(&current);
        let mut previous_resolved = previous.as_ref().map_or_else(
            || vec![(self.view_graph_id(), current)],
            |value| {
                value
                    .documents()
                    .map(|(id, graph)| (id.to_owned(), graph.clone()))
                    .collect()
            },
        );
        // Resolve output aliases in the baseline too, without changing the saved
        // Driver widths used to detect newly expanded Auto routes.
        let _ = self.resolve_output_identities(&mut previous_resolved);
        let before: Vec<_> = previous_resolved.iter().map(|(_, graph)| graph).collect();
        // A missing provider can make the old graph uncompileable. Node quotas
        // still apply; proposed graphs must always compile to count Auto routes.
        let mut before_usage =
            Usage::measure(&before, &compiler, context).unwrap_or_else(|_| Usage::nodes(&before));
        let mut proposed_usage = Usage::measure(&proposed, &compiler, context)?;
        if let Some(settings) = self.endpoint_settings() {
            // An old graph may contain unavailable plug-ins. Retain its node quotas;
            // valid proposed graphs must always resolve and count every route.
            let _ = before_usage.count_driver_aliases(&before, &compiler, context, &settings);
            proposed_usage.count_driver_aliases(&proposed, &compiler, context, &settings)?;
        }
        proposed_usage.compare(&before_usage)
    }
}

#[cfg(test)]
mod tests;

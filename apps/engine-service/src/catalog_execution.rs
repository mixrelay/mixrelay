//! Control-plane lowering of every catalog document into one running DSP composition.
//! Native inventory and verified application identity enter through explicit boundaries;
//! editor selection, display labels and guessed default-device equivalence do not.

use crate::graph_composition::{
    ComposedGraph, CompositionError, DeviceRegistry, ResolvedNodeIdentities, SharedOutput,
    SharedSource, compose, lower_document,
};
use std::collections::BTreeMap;
use tonenari_audio_graph::{
    AudioDeviceInputConfig, AudioDeviceOutputConfig, CompileContext, GraphCompiler, GraphDocument,
    NodeConfig, NodeSpec, VirtualEndpointId, VirtualEndpointSpec,
};
use tonenari_audio_io::{
    ApplicationCaptureRequest, AudioDeviceDescriptor, DeviceDirection, DeviceSelector,
};

/// Adapter-owned application identity resolution. The canonical request must have zero
/// gain and an actual supported channel width. Different aliases may share an ID only
/// after native resolution establishes that they capture the same signal and policy.
pub trait ApplicationIdentityResolver {
    /// Resolves one authored request without opening another capture owner.
    /// # Errors
    /// Rejects unavailable/ambiguous identity, unsupported policy and stale PID selections.
    fn resolve(
        &self,
        request: &ApplicationCaptureRequest,
    ) -> Result<SharedSource, CompositionError>;
}

/// One coherent control-plane input snapshot, independent of any selected editor graph.
#[derive(Clone, Copy)]
pub struct CatalogExecutionContext<'a> {
    pub revision: u64,
    pub compile: CompileContext,
    pub compiler: &'a GraphCompiler,
    /// Actual native device inventory, e.g. the result of macOS device enumeration.
    pub devices: &'a [AudioDeviceDescriptor],
    pub applications: &'a dyn ApplicationIdentityResolver,
}

/// Composes all supplied catalog documents without mutating their authored contents.
/// Empty catalogs produce an empty plan; no default device or source is synthesized.
/// # Errors
/// Rejects conflicting device definitions, unresolved physical/application identities,
/// incompatible shared-owner policies, invalid references and inter-graph feedback.
pub fn compose_catalog(
    documents: &[(String, GraphDocument)],
    context: CatalogExecutionContext<'_>,
) -> Result<ComposedGraph, CompositionError> {
    validate_size(documents, context.compile)?;
    let mut registry = RegistryInputs::default();
    let mut identities = Vec::with_capacity(documents.len());
    for (_, document) in documents {
        identities.push(registry.resolve(document, &context)?);
    }
    let registry = DeviceRegistry::new(registry.devices.into_values().collect())?
        .with_sources(registry.sources.into_values().collect())?
        .with_outputs(registry.outputs.into_values().collect())?;
    let authored = documents
        .iter()
        .zip(&identities)
        .map(|((_, document), identities)| lower_document(document, &registry, identities))
        .collect::<Result<Vec<_>, _>>()?;
    let fragments = documents
        .iter()
        .zip(&authored)
        .map(|((id, _), authored)| authored.as_fragment(id))
        .collect::<Vec<_>>();
    compose(
        &registry,
        &fragments,
        context.revision,
        context.compile,
        context.compiler,
    )
}

fn validate_size(
    documents: &[(String, GraphDocument)],
    context: CompileContext,
) -> Result<(), CompositionError> {
    let nodes = documents.iter().try_fold(0_usize, |total, (_, graph)| {
        total.checked_add(graph.nodes.len())
    });
    let edges = documents.iter().try_fold(0_usize, |total, (_, graph)| {
        total.checked_add(graph.edges.len())
    });
    if documents.len() > 256
        || nodes.is_none_or(|count| count > context.limits.max_nodes)
        || edges.is_none_or(|count| count > context.limits.max_edges)
    {
        return Err(invalid(
            "catalog exceeds aggregate graph compilation limits",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct RegistryInputs {
    devices: BTreeMap<VirtualEndpointId, VirtualEndpointSpec>,
    sources: BTreeMap<String, SharedSource>,
    outputs: BTreeMap<String, SharedOutput>,
}
impl RegistryInputs {
    fn resolve(
        &mut self,
        document: &GraphDocument,
        context: &CatalogExecutionContext<'_>,
    ) -> Result<ResolvedNodeIdentities, CompositionError> {
        let mut identities = ResolvedNodeIdentities::default();
        for node in &document.nodes {
            match &node.config {
                NodeConfig::VirtualEndpoint(config) => {
                    insert_same(&mut self.devices, config.id.clone(), config.spec.clone())?;
                }
                NodeConfig::ApplicationCapture(request) if request.requires_reselection => {
                    // A restored process identifier is intentionally disconnected until
                    // the user selects a live process. It owns no native capture resource.
                }
                NodeConfig::AudioDeviceInput(_) | NodeConfig::ApplicationCapture(_) => {
                    let source = resolve_source(node, context)?;
                    identities
                        .sources
                        .insert(node.id.clone(), source.id.clone());
                    insert_same(&mut self.sources, source.id.clone(), source)?;
                }
                NodeConfig::AudioDeviceOutput(config) => {
                    let output = resolve_output(config, context.devices)?;
                    identities
                        .outputs
                        .insert(node.id.clone(), output.id.clone());
                    insert_same(&mut self.outputs, output.id.clone(), output)?;
                }
                _ => {}
            }
        }
        Ok(identities)
    }
}

fn resolve_source(
    node: &NodeSpec,
    context: &CatalogExecutionContext<'_>,
) -> Result<SharedSource, CompositionError> {
    match &node.config {
        NodeConfig::AudioDeviceInput(config) => {
            let device = config
                .selector
                .resolve(
                    context.devices,
                    DeviceDirection::Input,
                    config.output_channels,
                )
                .map_err(|error| invalid(error.to_string()))?;
            Ok(SharedSource {
                id: physical_id(&device.uid),
                config: NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                    selector: DeviceSelector::PersistentUid {
                        uid: device.uid.clone(),
                    },
                    output_channels: supported_width(device.input_channels)?,
                    gain_db: 0.0,
                }),
            })
        }
        NodeConfig::ApplicationCapture(request) => {
            let mut resolved = context.applications.resolve(request)?;
            if !matches!(resolved.config, NodeConfig::ApplicationCapture(_)) {
                return Err(invalid(
                    "application resolver returned a non-application source",
                ));
            }
            resolved.id = format!("application:{}:{}", resolved.id.len(), resolved.id);
            Ok(resolved)
        }
        _ => Err(invalid("node is not a native capture source")),
    }
}

fn resolve_output(
    config: &AudioDeviceOutputConfig,
    devices: &[AudioDeviceDescriptor],
) -> Result<SharedOutput, CompositionError> {
    let device = config
        .selector
        .resolve(devices, DeviceDirection::Output, config.input_channels)
        .map_err(|error| invalid(error.to_string()))?;
    Ok(SharedOutput {
        id: physical_id(&device.uid),
        config: AudioDeviceOutputConfig {
            selector: DeviceSelector::PersistentUid {
                uid: device.uid.clone(),
            },
            input_channels: supported_width(device.output_channels)?,
        },
    })
}

fn physical_id(uid: &str) -> String {
    format!("device:{}:{uid}", uid.len())
}

fn supported_width(actual: u32) -> Result<u16, CompositionError> {
    // A wide interface remains one native owner, with the product's first 64 channels
    // available to references; a requested width was already checked against reality.
    u16::try_from(actual.min(64))
        .ok()
        .filter(|width| *width != 0)
        .ok_or_else(|| invalid("native device has no supported channels"))
}

fn insert_same<K: Ord, V: PartialEq>(
    entries: &mut BTreeMap<K, V>,
    key: K,
    value: V,
) -> Result<(), CompositionError> {
    match entries.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
        }
        std::collections::btree_map::Entry::Occupied(entry) if entry.get() != &value => {
            return Err(invalid(
                "conflicting canonical definitions for one native identity",
            ));
        }
        std::collections::btree_map::Entry::Occupied(_) => {}
    }
    Ok(())
}

fn invalid(reason: impl Into<String>) -> CompositionError {
    CompositionError::Invalid(reason.into())
}

#[cfg(test)]
#[path = "catalog_execution_tests.rs"]
mod tests;

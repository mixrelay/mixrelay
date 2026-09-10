//! Catalog-owned device identity. Graphs carry editable snapshots; accepting one
//! device edit updates every reference in the same catalog transaction.
use std::collections::{BTreeMap, BTreeSet};

use mixrelay_audio_graph::{GraphDocument, NodeConfig, VirtualEndpointId, VirtualEndpointSpec};
use mixrelay_audio_io::{VirtualEndpointDeviceSettings, VirtualEndpointSettings};
use mixrelay_control_protocol::ProtocolError;

use super::{Catalog, EngineGraphController, invalid};

#[cfg(test)]
#[path = "endpoint_observation_tests.rs"]
mod observation_tests;

fn specs(
    document: &GraphDocument,
) -> Result<BTreeMap<VirtualEndpointId, VirtualEndpointSpec>, ProtocolError> {
    let mut devices = BTreeMap::new();
    for node in &document.nodes {
        if let NodeConfig::VirtualEndpoint(config) = &node.config
            && let Some(previous) = devices.insert(config.id.clone(), config.spec.clone())
            && previous != config.spec
        {
            return Err(invalid("conflicting device definitions in one graph"));
        }
    }
    Ok(devices)
}

impl Catalog {
    /// A view cannot create a second native owner by copying a device snapshot.
    pub(super) fn device_specs(
        &self,
    ) -> Result<BTreeMap<VirtualEndpointId, VirtualEndpointSpec>, ProtocolError> {
        let mut devices = BTreeMap::new();
        for (_, document) in self.documents() {
            for (id, spec) in specs(document)? {
                if let Some(previous) = devices.insert(id, spec.clone())
                    && previous != spec
                {
                    return Err(invalid("device definitions disagree across graphs"));
                }
            }
        }
        Ok(devices)
    }

    pub(super) fn reconcile_device_edits(&mut self, previous: &Self) -> Result<(), ProtocolError> {
        let registered = previous.device_specs()?;
        let before = specs(&previous.active().document)?;
        let edited = specs(&self.active().document)?;
        for (id, spec) in &edited {
            if let Some(canonical) = registered.get(id)
                && canonical != spec
                && !before.contains_key(id)
            {
                return Err(invalid(
                    "new reference disagrees with existing device settings",
                ));
            }
        }
        // Device-level settings propagate; reference gain, visible sides, cables
        // and layout stay graph-local. The final reference controls native lifetime.
        for graph in &mut self.graphs {
            for node in &mut graph.document.nodes {
                if let NodeConfig::VirtualEndpoint(config) = &mut node.config
                    && let Some(spec) = edited.get(&config.id)
                {
                    config.spec.clone_from(spec);
                    node.label = Some(spec.display_name.clone());
                }
            }
        }
        self.device_specs()?;
        Ok(())
    }
}

impl EngineGraphController {
    /// Builds a single observed-rate snapshot for session publication or startup recall.
    pub(crate) fn observed_catalog_candidate(
        &self,
        document: &mut GraphDocument,
    ) -> Result<Option<Catalog>, ProtocolError> {
        let mut catalog = self.catalog_candidate(document);
        self.observe_endpoint_catalog(document, &mut catalog)?;
        if let Some(candidate) = &mut catalog {
            candidate.update_document(document.clone());
        }
        Ok(catalog)
    }

    /// Reads each catalog-owned native clock once, then updates only rate metadata
    /// in the caller's unpublished candidate. The caller holds the mutation lock;
    /// publication and persistence remain the normal graph transaction's responsibility.
    ///
    /// # Errors
    /// Rejects invalid native rates without changing either candidate. An unavailable
    /// readback leaves that device's metadata unchanged, never inventing a clock.
    pub(crate) fn observe_endpoint_catalog(
        &self,
        document: &mut GraphDocument,
        catalog: &mut Option<Catalog>,
    ) -> Result<bool, ProtocolError> {
        let mut identities = BTreeSet::new();
        collect_identities(document, &mut identities);
        if let Some(candidate) = catalog.as_ref() {
            for (_, graph) in candidate.documents() {
                collect_identities(graph, &mut identities);
            }
        }
        let mut rates = BTreeMap::new();
        for id in identities {
            let Ok(rate) = self.endpoint_backend.sample_rate(&id) else {
                continue;
            };
            if rate == 0
                || !self
                    .endpoint_backend
                    .capabilities()
                    .supported_sample_rates
                    .contains(&rate)
            {
                return Err(invalid("native device reported an unsupported sample rate"));
            }
            rates.insert(id, rate);
        }
        let mut changed = apply_observed_rates(document, &rates);
        if let Some(candidate) = catalog.as_mut() {
            for (_, graph) in candidate.documents_mut() {
                changed |= apply_observed_rates(graph, &rates);
            }
        }
        Ok(changed)
    }

    pub(crate) fn endpoint_settings(&self) -> Option<VirtualEndpointSettings> {
        let mut settings = self.endpoint_backend.settings()?;
        let native: BTreeMap<_, _> = settings
            .devices
            .drain(..)
            .map(|device| (device.spec.id.clone(), device))
            .collect();
        let catalog = self.catalog.lock();
        let devices = match catalog.as_ref().and_then(|store| store.value.as_ref().ok()) {
            Some(value) => value.device_specs().ok()?,
            None => specs(&self.graph.read()).ok()?,
        };
        settings.devices = devices
            .into_values()
            .map(|spec| {
                let observed_sample_rate = self.endpoint_backend.sample_rate(&spec.id).ok();
                let capability = native.get(&spec.id);
                VirtualEndpointDeviceSettings {
                    native_device_ids: capability
                        .map_or_else(Vec::new, |device| device.native_device_ids.clone()),
                    supported_sample_rates: capability.map_or_else(
                        || spec.supported_sample_rates.clone(),
                        |device| device.supported_sample_rates.clone(),
                    ),
                    can_set_sample_rate: capability
                        .is_some_and(|device| device.can_set_sample_rate),
                    spec,
                    observed_sample_rate,
                }
            })
            .collect();
        Some(settings)
    }

    pub(crate) fn reconcile_catalog_devices(
        &self,
        candidate: &mut Catalog,
    ) -> Result<(), ProtocolError> {
        let catalog = self.catalog.lock();
        if let Some(previous) = catalog.as_ref().and_then(|store| store.value.as_ref().ok()) {
            candidate.reconcile_device_edits(previous)?;
        }
        Ok(())
    }
}

fn collect_identities(document: &GraphDocument, identities: &mut BTreeSet<VirtualEndpointId>) {
    for node in &document.nodes {
        if let NodeConfig::VirtualEndpoint(config) = &node.config {
            identities.insert(config.id.clone());
        }
    }
}

fn apply_observed_rates(
    document: &mut GraphDocument,
    rates: &BTreeMap<VirtualEndpointId, u32>,
) -> bool {
    let mut changed = false;
    for node in &mut document.nodes {
        if let NodeConfig::VirtualEndpoint(config) = &mut node.config
            && let Some(&rate) = rates.get(&config.id)
        {
            changed |= config.default_sample_rate != rate;
            config.default_sample_rate = rate;
            if !config.supported_sample_rates.contains(&rate) {
                config.supported_sample_rates.push(rate);
                config.supported_sample_rates.sort_unstable();
                changed = true;
            }
        }
    }
    changed
}

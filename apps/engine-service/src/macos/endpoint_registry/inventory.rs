//! Whole-inventory transaction. One reservation excludes mutation until commit/cancel;
//! unchanged transports retain their incarnation and every removed transport is retired.
use super::{
    CallbackEndpoint, EndpointId, EndpointRegistry, EndpointSpec, Mapping, exhausted,
    require_unreserved,
};
use mixrelay_macos_transport::control_inventory::MAX_ENTRIES;
use std::{collections::BTreeMap, io, sync::Arc};

/// Prepared complete native inventory. Dropping cancels its unpublished changes;
/// callbacks can pin its mappings before a single audio-boundary publication.
#[must_use = "dropping the transaction cancels its inventory changes"]
pub struct PreparedInventory<'a> {
    registry: &'a EndpointRegistry,
    token: Option<u64>,
    next_generation: u64,
    desired: BTreeMap<EndpointId, Arc<Mapping>>,
}

impl EndpointRegistry {
    /// Prepares exactly the referenced devices; an empty slice removes every endpoint.
    /// Existing equal specs reuse storage. Allocation and validation precede reservation.
    /// # Errors
    /// Rejects duplicate identities, invalid geometry, conflicting mutation, exhausted
    /// generations, or mapping allocation failure without changing active inventory.
    pub fn prepare_inventory(&self, specs: &[EndpointSpec]) -> io::Result<PreparedInventory<'_>> {
        self.prepare_inventory_change(specs, None)
    }

    /// Prepares an inventory, optionally replacing one exhausted mapping regardless of geometry.
    /// # Errors
    /// Rejects an unknown replacement identity and the same failures as `prepare_inventory`.
    pub fn prepare_inventory_change(
        &self,
        specs: &[EndpointSpec],
        replacement: Option<EndpointId>,
    ) -> io::Result<PreparedInventory<'_>> {
        if specs.len() > MAX_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "endpoint inventory is full",
            ));
        }
        let mut state = self.state.lock();
        require_unreserved(&state)?;
        if replacement.is_some_and(|id| {
            !state.active.contains_key(&id) || !specs.iter().any(|spec| spec.id == id)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "replacement endpoint is absent",
            ));
        }
        let next_generation = state.generation.checked_add(1).ok_or_else(exhausted)?;
        let token = state.next_token;
        state.next_token = token.checked_add(1).ok_or_else(exhausted)?;
        let mut desired = BTreeMap::new();
        for spec in specs {
            spec.validate()?;
            if desired.contains_key(&spec.id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "duplicate endpoint identity",
                ));
            }
            let mapping = if let Some(current) = state
                .active
                .get(&spec.id)
                .filter(|mapping| mapping.spec() == spec && replacement != Some(spec.id))
            {
                Arc::clone(current)
            } else {
                let generation = state.next_token;
                state.next_token = generation.checked_add(1).ok_or_else(exhausted)?;
                Arc::new(Mapping::new(spec.clone(), generation)?)
            };
            desired.insert(spec.id, mapping);
        }
        state.inventory_pending = Some(token);
        Ok(PreparedInventory {
            registry: self,
            token: Some(token),
            next_generation,
            desired,
        })
    }
}

impl<'a> PreparedInventory<'a> {
    /// Pins each provisional mapping on the control thread for callback publication.
    #[must_use]
    pub fn callback_pins(&self) -> Vec<CallbackEndpoint<'a>> {
        self.desired
            .values()
            .map(|mapping| CallbackEndpoint {
                mapping: Arc::clone(mapping),
                _registry: self.registry,
                _pinned: std::marker::PhantomPinned,
            })
            .collect()
    }

    /// Replaces the entire authoritative inventory under one registry lock/generation.
    /// The reservation excludes every conflicting mutation between preparation and commit.
    /// # Errors
    /// A violated reservation leaves the active inventory unchanged.
    pub fn commit(mut self) -> io::Result<()> {
        let mut state = self.registry.state.lock();
        if self.token.is_none() || state.inventory_pending != self.token {
            return Err(io::Error::other("endpoint inventory reservation was lost"));
        }
        let previous = std::mem::replace(&mut state.active, std::mem::take(&mut self.desired));
        for (id, mapping) in previous {
            if !state
                .active
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &mapping))
            {
                state.retired.push(mapping);
            }
        }
        state.generation = self.next_generation;
        state.inventory_pending = None;
        self.token = None;
        Ok(())
    }
}

impl Drop for PreparedInventory<'_> {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        let mut state = self.registry.state.lock();
        if state.inventory_pending == Some(token) {
            state.inventory_pending = None;
        }
        for (id, mapping) in std::mem::take(&mut self.desired) {
            if !state
                .active
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &mapping))
            {
                state.retired.push(mapping);
            }
        }
    }
}

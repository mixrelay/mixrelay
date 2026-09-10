//! Control-owned virtual endpoint transports, independent of graph identity.
//!
//! A callback receives an immutable pinned handle before rendering starts. It
//! never discovers an endpoint through a global pointer or locks this registry.
//! Retired mappings remain owned here until explicit control-thread collection.
#[path = "endpoint_registry/inventory.rs"]
mod inventory;
#[path = "endpoint_registry/mapping.rs"]
mod mapping;
pub use inventory::PreparedInventory;
#[cfg(test)]
#[path = "endpoint_registry/tests.rs"]
mod tests;

use mapping::Mapping;
use mixrelay_macos_transport::control_inventory::{InventoryEntry, InventorySnapshot, NAME_BYTES};
use mixrelay_macos_transport::{
    endpoint_transport::EndpointTransport, shared_audio_buffer::SharedAudioBuffer,
};
use parking_lot::Mutex;
use std::{collections::BTreeMap, ffi::CString, io, os::fd::OwnedFd, sync::Arc};

pub type EndpointId = [u8; 16];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointSpec {
    pub id: EndpointId,
    pub display_name: String,
    pub sample_rate: u32,
    pub capacity_frames: u32,
    pub render_channels: u32,
    pub capture_channels: u32,
}

impl EndpointSpec {
    fn validate(&self) -> io::Result<()> {
        if self.id == [0; 16]
            || self.display_name.is_empty()
            || self.display_name.len() >= NAME_BYTES
            || CString::new(self.display_name.as_str()).is_err()
            || !mixrelay_audio_io::formats::SAMPLE_RATES.contains(&self.sample_rate)
            || self.render_channels > 64
            || self.capture_channels > 64
            || self.render_channels + self.capture_channels == 0
            || !self.capacity_frames.is_power_of_two()
            || !(32..=1 << 20).contains(&self.capacity_frames)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid endpoint geometry or identity",
            ));
        }
        Ok(())
    }
}

struct State {
    active: BTreeMap<EndpointId, Arc<Mapping>>,
    retired: Vec<Arc<Mapping>>,
    next_token: u64,
    generation: u64,
    inventory_pending: Option<u64>,
}

pub struct EndpointRegistry {
    state: Mutex<State>,
}

impl Default for EndpointRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EndpointRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                active: BTreeMap::new(),
                retired: Vec::new(),
                next_token: 1,
                generation: 1,
                inventory_pending: None,
            }),
        }
    }

    /// Pins on control before passing the handle into a callback. The lifetime
    /// prevents registry destruction while any callback can access its mapping.
    #[must_use]
    pub fn callback(&self, id: EndpointId) -> Option<CallbackEndpoint<'_>> {
        self.state
            .lock()
            .active
            .get(&id)
            .map(|mapping| CallbackEndpoint {
                mapping: Arc::clone(mapping),
                _registry: self,
                _pinned: std::marker::PhantomPinned,
            })
    }

    /// The only normal retirement reclamation point. Run on the control thread.
    pub fn collect(&self) -> usize {
        let mut state = self.state.lock();
        let before = state.retired.len();
        state
            .retired
            .retain(|mapping| Arc::strong_count(mapping) != 1);
        before - state.retired.len()
    }

    /// Duplicates every descriptor before exposing an authoritative snapshot.
    /// # Errors
    /// Any descriptor duplication failure drops all copies and returns no snapshot.
    pub fn snapshot(&self) -> io::Result<OwnedInventory> {
        let state = self.state.lock();
        let mut snapshot = InventorySnapshot {
            generation: state.generation,
            ..InventorySnapshot::default()
        };
        let mut descriptors = Vec::with_capacity(state.active.len());
        for (index, mapping) in state.active.values().enumerate() {
            let (entry, descriptor) = mapping.inventory_entry()?;
            snapshot.entries[index] = entry;
            descriptors.push(descriptor);
        }
        snapshot.count = descriptors.len();
        Ok(OwnedInventory {
            snapshot,
            _descriptors: descriptors,
        })
    }
}

fn exhausted() -> io::Error {
    io::Error::other("endpoint generation exhausted; no generation is reused")
}

fn require_unreserved(state: &State) -> io::Result<()> {
    if state.inventory_pending.is_some() {
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "endpoint inventory transaction is reserved",
        ))
    } else {
        Ok(())
    }
}

/// No lookup, lock, allocation or final mapping destruction occurs on drop.
/// Even after removal, the registry owns the retired mapping until `collect`.
pub struct CallbackEndpoint<'registry> {
    mapping: Arc<Mapping>,
    _registry: &'registry EndpointRegistry,
    // A service may publish this handle's address to callbacks. Once pinned,
    // moving its fields out must remain impossible until the readers drain.
    _pinned: std::marker::PhantomPinned,
}

impl CallbackEndpoint<'_> {
    #[must_use]
    pub fn spec(&self) -> &EndpointSpec {
        self.mapping.spec()
    }
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.mapping.generation()
    }
    #[must_use]
    pub fn transport(&self) -> *mut EndpointTransport {
        self.mapping.transport()
    }
    #[must_use]
    pub fn render_buffer(&self) -> *mut SharedAudioBuffer {
        self.mapping.render_buffer()
    }
    #[must_use]
    pub fn capture_buffer(&self) -> *mut SharedAudioBuffer {
        self.mapping.capture_buffer()
    }
}

/// The ABI contains borrowed FD numbers; this owner keeps all of them valid
/// across registry replacement/removal until the XPC codec duplicates them.
pub struct OwnedInventory {
    snapshot: InventorySnapshot,
    _descriptors: Vec<OwnedFd>,
}
impl OwnedInventory {
    #[must_use]
    pub fn as_abi(&self) -> &InventorySnapshot {
        &self.snapshot
    }
    #[must_use]
    pub fn entries(&self) -> &[InventoryEntry] {
        &self.snapshot.entries[..self.snapshot.count]
    }
}

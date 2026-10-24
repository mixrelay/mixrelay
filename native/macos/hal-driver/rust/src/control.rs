//! One process-lifetime owner for the HAL registry and its serial control resources.
//! No control-state borrow crosses a registry call that can notify the HAL host.

use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};

use crate::{
    abi::HostOps,
    control_session::{ControlSession, TickAction},
    inventory::{
        mr_hal_core_control_connected, mr_hal_core_control_message, mr_hal_core_replacement_request,
    },
    native::mr_hal_core_create_native,
    registry::Registry,
};

mod native;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Inventory,
    Acknowledged { generation: u64 },
    Replacement { endpoint: [u8; 16], generation: u64 },
}

/// The adapter owns queue/timer handles; each connection value owns its socket.
/// Methods enqueue asynchronous events, never call back into the owner inline.
trait Backend {
    type Connection;
    fn connect(&self, owner: *const c_void, generation: u64) -> Option<Self::Connection>;
    fn resume(&self, connection: &Self::Connection);
    fn send(&self, connection: &Self::Connection, request: Request);
}

struct Control<B: Backend> {
    registry: Box<Registry>,
    session: RefCell<ControlSession>,
    connection: RefCell<Option<B::Connection>>,
    backend: B,
    acknowledged: Cell<(u64, u64)>,
}

impl<B: Backend> Control<B> {
    fn new(registry: Box<Registry>, backend: B) -> Self {
        Self {
            registry,
            session: RefCell::new(ControlSession::default()),
            connection: RefCell::new(None),
            backend,
            acknowledged: Cell::new((0, 0)),
        }
    }

    fn registry_pointer(&self) -> *mut Registry {
        ptr::from_ref(self.registry.as_ref()).cast_mut()
    }

    fn open(&self) {
        let generation = self.session.borrow_mut().begin();
        if generation == 0 {
            return;
        }
        // Fence callbacks before cancel/release can enqueue a final stale event.
        self.connection.borrow_mut().take();
        // SAFETY: this owner retains the registry; the only control queue invokes
        // open and event delivery. The state borrow above has already ended.
        let accepted =
            unsafe { mr_hal_core_control_connected(self.registry_pointer(), generation) };
        if !self.session.borrow_mut().authorize(generation, accepted) {
            return;
        }
        let connection = self.backend.connect(ptr::from_ref(self).cast(), generation);
        if !self
            .session
            .borrow_mut()
            .opened(generation, connection.is_some())
        {
            return;
        }
        *self.connection.borrow_mut() = connection;
        if let Some(connection) = self.connection.borrow().as_ref() {
            self.backend.resume(connection);
            self.backend.send(connection, Request::Inventory);
        }
    }

    fn tick(&self) {
        // Process death releases the OS lease even if the driver service is stalled.
        // Filesystem work is confined to this serial control queue, never HAL IO.
        if !mixrelay_macos_transport::app_lifecycle::is_owned(
            mixrelay_macos_transport::app_lifecycle::ENGINE_LEASE,
        )
        .unwrap_or(false)
        {
            self.registry.clear_latencies();
        }
        self.registry.tick();
        let action = self.session.borrow_mut().tick();
        self.request_replacement();
        let authority = self.session.borrow().authority();
        if let Some(generation) = crate::inventory::acknowledged(&self.registry, authority)
            && self.acknowledged.get() != (authority, generation)
        {
            self.send(Request::Acknowledged { generation });
            self.acknowledged.set((authority, generation));
        }
        match action {
            TickAction::Maintain => {}
            TickAction::Reconnect => self.open(),
            TickAction::RefreshInventory => self.send(Request::Inventory),
        }
    }

    fn send(&self, request: Request) {
        if let Some(connection) = self.connection.borrow().as_ref() {
            self.backend.send(connection, request);
        }
    }

    fn request_replacement(&self) {
        let authority = self.session.borrow().authority();
        if authority == 0 {
            return;
        }
        let mut endpoint = [0; 16];
        let mut generation = 0;
        // SAFETY: the registry is retained, both stack outputs are disjoint,
        // and this query runs only on the serial control queue.
        if unsafe {
            mr_hal_core_replacement_request(
                self.registry_pointer(),
                authority,
                &raw mut endpoint,
                &raw mut generation,
            )
        } {
            self.send(Request::Replacement {
                endpoint,
                generation,
            });
        }
    }

    fn disconnected(&self, generation: u64) {
        let accepted = self.session.borrow().accepts(generation);
        self.session.borrow_mut().disconnected(generation);
        if accepted {
            self.registry.clear_latencies();
        }
    }

    /// The XPC callback retains a live event until this method returns.
    unsafe fn message(&self, generation: u64, message: *mut c_void) -> bool {
        let accepted = self.session.borrow().accepts(generation);
        // SAFETY: the native event callback retains message for this call and
        // its process-lifetime owner retains registry. No RefCell guard survives.
        accepted
            && unsafe { mr_hal_core_control_message(self.registry_pointer(), generation, message) }
    }
}

/// Publishes a registry only after both dispatch resources have been acquired.
/// The HAL factory has no shutdown callback: a successful owner lives until exit,
/// independently of COM reference counts. Failed initialization publishes nothing.
///
/// # Safety
/// `host` is null or points to readable host operations whose context/callbacks
/// remain valid for the HAL process lifetime. The SDK serializes initialization.
/// `output` is null or the exclusively writable, process-lifetime COM registry
/// slot. It must not overlap host operations or any already published owner.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_control_initialize(
    host: *const HostOps,
    output: *mut *mut Registry,
) -> bool {
    // SAFETY: Initialize supplies its stack host-operations record for this call.
    let Some(host) = (unsafe { host.as_ref() }) else {
        return false;
    };
    if output.is_null() {
        return false;
    }
    let Some(backend) = native::Native::new() else {
        return false;
    };
    // SAFETY: create copies host operations and transfers unique registry ownership.
    let registry = unsafe { mr_hal_core_create_native(ptr::from_ref(host)) };
    if registry.is_null() {
        return false;
    }
    // SAFETY: the non-null allocation above has no other owner or published callback.
    let owner = Box::new(Control::new(unsafe { Box::from_raw(registry) }, backend));
    let owner = Box::leak(owner);
    // SAFETY: Initialize exclusively owns the static COM registry slot. Publish
    // it before queue submission, so even an immediate host callback sees it.
    unsafe { output.write(registry) };
    owner.backend.publish(owner);
    true
}

#[cfg(test)]
mod tests;

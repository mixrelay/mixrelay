//! Registered client ownership and first-start/final-stop native transitions.
use super::Registry;
use crate::{
    abi::{CAPACITY, INVALID, NATIVE_ERROR, NOT_FOUND, OK, RUNNING},
    device::lock,
};
use std::sync::atomic::Ordering;

impl Registry {
    pub(crate) fn client(&self, id: u32, client: u32, add: bool) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        let mut stopped = false;
        let frozen = device.freeze();
        if add {
            if !device.alive.load(Ordering::Acquire) {
                return NOT_FOUND;
            }
            if control.clients.contains_key(&client) {
                return INVALID;
            }
            // SAFETY: control and freeze retain this live runtime and exclude IO readers.
            if !unsafe {
                self.native.update_client(
                    device.runtime.load(Ordering::Acquire),
                    client,
                    crate::io_outcome::ClientChange::Register,
                )
            } {
                return NATIVE_ERROR;
            }
            control.clients.insert(client, false);
            device.input_latency.add(client);
        } else {
            match control.clients.get(&client) {
                Some(false) => {
                    control.clients.remove(&client);
                }
                Some(true) => {
                    // A client process can disappear without a balanced StopIO.
                    // Its removal releases only that client's runtime claim.
                    let count = device.running.load(Ordering::Acquire) - 1;
                    if device.alive.load(Ordering::Acquire) {
                        let runtime = device.runtime.load(Ordering::Acquire);
                        if count == 0 {
                            // SAFETY: control and freeze retain the live, drained runtime.
                            unsafe { (self.native.stop)(runtime) };
                        } else {
                            // SAFETY: the same drained runtime retains the other clients.
                            unsafe { (self.native.remaining_client)(runtime) };
                        }
                    }
                    device.running.store(count, Ordering::Release);
                    control.clients.remove(&client);
                    stopped = count == 0;
                }
                None => return INVALID,
            }
        }
        if !add {
            if device.alive.load(Ordering::Acquire) {
                // SAFETY: control and freeze exclude callbacks during queue retirement.
                unsafe {
                    self.native.update_client(
                        device.runtime.load(Ordering::Acquire),
                        client,
                        crate::io_outcome::ClientChange::Remove,
                    )
                };
            }
            device.input_latency.remove(client);
        }
        drop(frozen);
        drop(control);
        drop(device);
        if stopped {
            self.host.changed(id, RUNNING);
        }
        if !add {
            self.reclaim();
        }
        if let Some(signal) = self.control_signal.get() {
            signal.notify();
        }
        OK
    }

    pub(crate) fn start(&self, id: u32, client: u32, now: u64) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        if !device.alive.load(Ordering::Acquire) {
            return NOT_FOUND;
        }
        let Some(running) = control.clients.get_mut(&client) else {
            return INVALID;
        };
        if *running {
            return INVALID;
        }
        let count = device.running.load(Ordering::Acquire);
        if count == u32::MAX {
            return CAPACITY;
        }
        let frozen = device.freeze();
        // SAFETY: control and freeze retain the live runtime before its first start.
        if count == 0
            && !unsafe { (self.native.start)(device.runtime.load(Ordering::Acquire), now) }
        {
            return NATIVE_ERROR;
        }
        *running = true;
        device.input_latency.active(client, true);
        device.running.store(count + 1, Ordering::Release);
        drop(frozen);
        drop(control);
        drop(device);
        if count == 0 {
            self.host.changed(id, RUNNING);
            if let Some(signal) = self.control_signal.get() {
                signal.notify();
            }
        }
        OK
    }

    pub(crate) fn stop(&self, id: u32, client: u32) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        let Some(running) = control.clients.get_mut(&client) else {
            return INVALID;
        };
        if !*running {
            return INVALID;
        }
        let frozen = device.freeze();
        let count = device.running.load(Ordering::Acquire) - 1;
        if device.alive.load(Ordering::Acquire) {
            let runtime = device.runtime.load(Ordering::Acquire);
            if count == 0 {
                // SAFETY: control and freeze retain the live, drained runtime.
                unsafe { (self.native.stop)(runtime) };
            } else {
                // SAFETY: the same drained runtime retains the other clients.
                unsafe { (self.native.remaining_client)(runtime) };
            }
        }
        *running = false;
        device.input_latency.active(client, false);
        device.running.store(count, Ordering::Release);
        drop(frozen);
        drop(control);
        drop(device);
        if count == 0 {
            self.host.changed(id, RUNNING);
        }
        OK
    }
}

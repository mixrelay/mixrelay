//! Registered client ownership and first-start/final-stop native transitions.
use super::Registry;
use crate::{
    abi::{CAPACITY, INVALID, NATIVE_ERROR, NOT_FOUND, OK, RUNNING},
    device::{Control, Device, Prepared, lock},
};
use std::sync::atomic::Ordering;

impl Registry {
    pub(crate) fn client(&self, id: u32, client: u32, add: bool) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        let changed = if add {
            self.add_client(&device, &mut control, client)
        } else {
            self.remove_client(&device, &mut control, client)
        };
        let stopped = match changed {
            Ok(stopped) => stopped,
            Err(status) => return status,
        };
        // A pending replacement must carry exactly the new client set.
        Self::prepare_attachment(&device, &mut control);
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

    fn add_client(&self, device: &Device, control: &mut Control, client: u32) -> Result<bool, i32> {
        if !device.alive.load(Ordering::Acquire) {
            return Err(NOT_FOUND);
        }
        if control.clients.contains_key(&client) {
            return Err(INVALID);
        }
        let runtime = device.runtime.load(Ordering::Acquire);
        // SAFETY: control retains this live runtime. Mapping the queue runs alongside IO
        // callbacks; only its publication below needs the freeze.
        let prepared = Prepared::new(
            unsafe { (self.native.prepare_client)(runtime, client) },
            self.native,
        )
        .ok_or(NATIVE_ERROR)?;
        let _frozen = device.freeze();
        // SAFETY: control and freeze retain this live runtime and exclude IO readers.
        if !unsafe {
            self.native.update_client(
                runtime,
                client,
                crate::io_outcome::ClientChange::Register,
                Some(prepared),
            )
        } {
            return Err(NATIVE_ERROR);
        }
        control.clients.insert(client, false);
        device.input_latency.add(client);
        Ok(false)
    }

    fn remove_client(
        &self,
        device: &Device,
        control: &mut Control,
        client: u32,
    ) -> Result<bool, i32> {
        let _frozen = device.freeze();
        let stopped = match control.clients.get(&client) {
            Some(false) => false,
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
                count == 0
            }
            None => return Err(INVALID),
        };
        control.clients.remove(&client);
        if device.alive.load(Ordering::Acquire) {
            // SAFETY: control and freeze exclude callbacks during queue retirement.
            unsafe {
                self.native.update_client(
                    device.runtime.load(Ordering::Acquire),
                    client,
                    crate::io_outcome::ClientChange::Remove,
                    None,
                )
            };
        }
        device.input_latency.remove(client);
        Ok(stopped)
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

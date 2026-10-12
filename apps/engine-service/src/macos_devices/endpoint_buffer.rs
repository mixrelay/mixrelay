//! Per-device I/O client settings owned by the process that creates its `IOProcs`.
use super::{
    MacDevices, backend_error,
    buffer_control::NativeBufferControl,
    buffer_transaction::{BufferControl, BufferPlan},
    native::Native,
};
use mixrelay_audio_io::{
    DeviceSelector, HostAudioBackendError, NativeBufferSettings, VirtualEndpointId,
};
use mixrelay_macos_transport::{
    device_io::{BufferInfo, buffer_info_for_uid, mr_device_io_buffer_info},
    endpoint_identity::{endpoint_uid, format_endpoint_id, parse_endpoint_id},
};
use std::{collections::BTreeMap, ffi::CString, sync::Arc};

fn selection_id(selector: &DeviceSelector) -> Option<String> {
    let DeviceSelector::PersistentUid { uid } = selector else {
        return None;
    };
    let hex = uid.strip_prefix("io.mixrelay.audio.endpoint.")?;
    if hex.len() != 32
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    let id = parse_endpoint_id(&format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))?;
    Some(format_endpoint_id(id))
}
pub(super) fn is_virtual_selection(selector: &DeviceSelector) -> bool {
    selection_id(selector).is_some()
}
fn uid(id: &VirtualEndpointId) -> Option<CString> {
    let parsed = parse_endpoint_id(id.as_str())?;
    if format_endpoint_id(parsed) != id.as_str() {
        return None;
    }
    CString::new(endpoint_uid(parsed)).ok()
}

fn owner_info(native: &Native) -> Option<BufferInfo> {
    let mut info = BufferInfo::default();
    // SAFETY: the Arc-retained Native owns its live allocation; output is aligned,
    // initialized and exclusively borrowed until the synchronous property read returns.
    unsafe { mr_device_io_buffer_info(native.pointer.as_ptr().cast(), &raw mut info) }
        .then_some(info)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "finite integral positive native buffer bounds checked before conversion"
)]
fn settings(
    info: &BufferInfo,
    active: bool,
    preferred: Option<u32>,
) -> Option<NativeBufferSettings> {
    if info.device == 0
        || info.frames == 0
        || !info.minimum.is_finite()
        || !info.maximum.is_finite()
        || info.minimum.fract() != 0.0
        || info.maximum.fract() != 0.0
        || !(1.0..=4096.0).contains(&info.minimum)
        || info.maximum < info.minimum
    {
        return None;
    }
    Some(NativeBufferSettings {
        current_frames: active.then_some(info.frames),
        preferred_frames: preferred,
        minimum_frames: info.minimum as u32,
        maximum_frames: info.maximum.min(4096.0) as u32,
        can_set: info.settable == 1 && info.minimum <= info.maximum.min(4096.0),
    })
}

impl MacDevices {
    pub(super) fn saved_endpoint_buffer(&self, selector: &DeviceSelector) -> Option<u32> {
        let id = selection_id(selector)?;
        self.endpoint_buffers
            .lock()
            .ok()?
            .as_ref()
            .ok()?
            .frames(&id)
    }
    pub(super) fn endpoint_buffer_settings(
        &self,
        id: &VirtualEndpointId,
    ) -> Option<NativeBufferSettings> {
        let uid = uid(id)?;
        let cache = self.cache.lock().ok()?;
        let owner = cache
            .values()
            .filter(|(spec, _)| selection_id(&spec.selector).as_deref() == Some(id.as_str()))
            .filter_map(|(_, weak)| weak.upgrade())
            .find(|device| device.ready());
        let preferred = self
            .endpoint_buffers
            .lock()
            .ok()?
            .as_ref()
            .ok()
            .and_then(|store| store.frames(id.as_str()));
        let info = match owner.as_ref() {
            Some(owner) if owner.native.group.is_none() => owner_info(&owner.native)?,
            Some(_) => return None, // Aggregate quantum cannot represent independent device intent.
            None => buffer_info_for_uid(&uid)?,
        };
        settings(&info, owner.is_some(), preferred)
    }
    pub(super) fn change_endpoint_buffer(
        &self,
        id: &VirtualEndpointId,
        expected: Option<u32>,
        expected_preferred: Option<u32>,
        frames: u32,
    ) -> Result<Option<String>, HostAudioBackendError> {
        self.require_preparation()?;
        let uid = uid(id).ok_or_else(|| backend_error("invalid virtual device identity"))?;
        // Serializes against recovery/owner replacement. No DSP plan or queue is replaced.
        let cache = self
            .cache
            .lock()
            .map_err(|_| backend_error("CoreAudio device cache poisoned"))?;
        if !self.handoff.idle() {
            return Err(backend_error("CoreAudio resource publication pending"));
        }
        let target = cache
            .values()
            .filter(|(spec, _)| selection_id(&spec.selector).as_deref() == Some(id.as_str()))
            .filter_map(|(_, weak)| weak.upgrade())
            .find(|device| device.ready());
        let info = match target.as_ref() {
            Some(device) if device.native.group.is_none() => owner_info(&device.native),
            Some(_) => None,
            None => buffer_info_for_uid(&uid),
        }
        .ok_or_else(|| backend_error("virtual device client buffer unavailable"))?;
        let capability = settings(&info, target.is_some(), None)
            .ok_or_else(|| backend_error("virtual device client buffer capability unavailable"))?;
        if capability.current_frames != expected || !capability.accepts(frames) {
            return Err(backend_error(
                "stale or unsupported virtual device client buffer",
            ));
        }
        let mut store = self
            .endpoint_buffers
            .lock()
            .map_err(|_| backend_error("client buffer preferences poisoned"))?;
        let preferred = store
            .as_ref()
            .ok()
            .and_then(|store| store.frames(id.as_str()));
        if preferred != expected_preferred {
            return Err(backend_error(
                "virtual device client buffer preference changed",
            ));
        }
        let owners: Vec<Arc<super::Device>> = cache
            .values()
            .filter_map(|(_, weak)| weak.upgrade())
            .filter(|device| device.ready())
            .collect();
        if target.is_some() {
            let observations = observe_owners(&owners).map_err(backend_error)?;
            if let Err(error) = apply_independent_buffer(
                &observations,
                info.device,
                frames,
                &mut NativeBufferControl,
            ) {
                if error.contains("rollback failed") {
                    self.status.fail_reconfiguration();
                }
                return Err(backend_error(error));
            }
            for device in &owners {
                if owner_info(&device.native).is_some_and(|readback| readback.device == info.device)
                {
                    device.native.request_endpoint_buffer(frames);
                }
            }
        }
        // Without an IOProc this is saved intent only. Connect applies and verifies it.
        Ok(match store.as_mut() {
            Ok(store) => store
                .save(id.as_str(), frames)
                .err()
                .map(|error| error.to_string()),
            Err(error) => Some(error.clone()),
        })
    }
}

fn observe_owners(owners: &[Arc<super::Device>]) -> Result<BTreeMap<u32, u32>, String> {
    let mut observed = BTreeMap::new();
    for owner in owners {
        let info = owner_info(&owner.native).ok_or("native I/O client buffer unavailable")?;
        if info.frames == 0
            || observed
                .insert(info.device, info.frames)
                .is_some_and(|old| old != info.frames)
        {
            return Err("native I/O buffer changed during observation".into());
        }
    }
    Ok(observed)
}

pub(super) fn restore_saved(
    target: &Arc<super::Device>,
    frames: u32,
    owners: &[Arc<super::Device>],
) -> Result<(), HostAudioBackendError> {
    let info = owner_info(&target.native)
        .ok_or_else(|| backend_error("saved client buffer owner unavailable"))?;
    if !settings(&info, true, Some(frames)).is_some_and(|settings| settings.accepts(frames)) {
        return Err(backend_error(
            "saved client buffer is unsupported by this native device",
        ));
    }
    apply_independent_buffer(
        &observe_owners(owners).map_err(backend_error)?,
        info.device,
        frames,
        &mut NativeBufferControl,
    )
    .map_err(backend_error)?;
    target.native.request_endpoint_buffer(frames);
    Ok(())
}

/// Reject host behavior that aliases another native client's buffer to this edit.
fn apply_independent_buffer(
    observed: &BTreeMap<u32, u32>,
    target: u32,
    frames: u32,
    control: &mut impl BufferControl,
) -> Result<(), String> {
    let previous = *observed
        .get(&target)
        .ok_or("target I/O client observation unavailable")?;
    let plan = BufferPlan::prepare([(target, previous)], frames)
        .map_err(|e| format!("client buffer preparation failed: {e:?}"))?;
    let applied = plan.apply(control);
    if applied.is_ok()
        && observed
            .iter()
            .all(|(&id, &old)| control.current(id) == if id == target { frames } else { old })
    {
        return Ok(());
    }
    let mut restored = true;
    for (&id, &old) in observed {
        let current = control.current(id);
        if current != old {
            restored &= current == frames && control.change(id, current, old);
        }
    }
    restored &= observed
        .iter()
        .all(|(&id, &old)| control.current(id) == old);
    Err(if restored {
        "native client buffer change rejected; previous settings restored"
    } else {
        "client buffer independence check failed; rollback failed"
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Control {
        values: BTreeMap<u32, u32>,
        shared: bool,
        fail_restore: bool,
    }
    impl BufferControl for Control {
        fn current(&mut self, id: u32) -> u32 {
            self.values[&id]
        }
        fn change(&mut self, id: u32, expected: u32, frames: u32) -> bool {
            if self.values[&id] != expected || (self.fail_restore && frames == 512) {
                return false;
            }
            if self.shared {
                for value in self.values.values_mut() {
                    *value = frames;
                }
            } else {
                self.values.insert(id, frames);
            }
            true
        }
    }
    #[test]
    fn one_endpoint_edit_preserves_other_clients_and_shared_host_behavior_rolls_back() {
        let observed = BTreeMap::from([(1, 512), (2, 512)]);
        let mut control = Control {
            values: observed.clone(),
            shared: false,
            fail_restore: false,
        };
        apply_independent_buffer(&observed, 1, 128, &mut control).unwrap();
        assert_eq!(control.values, BTreeMap::from([(1, 128), (2, 512)]));
        control.values = observed.clone();
        control.shared = true;
        assert!(
            apply_independent_buffer(&observed, 1, 128, &mut control)
                .unwrap_err()
                .contains("restored")
        );
        assert_eq!(control.values, observed);
        control.fail_restore = true;
        assert!(
            apply_independent_buffer(&observed, 1, 128, &mut control)
                .unwrap_err()
                .contains("rollback failed")
        );
        assert_eq!(control.values, BTreeMap::from([(1, 128), (2, 128)]));
    }
    #[test]
    fn noncanonical_endpoint_identity_cannot_create_a_second_preference_key() {
        let canonical = VirtualEndpointId::from("abcdef01-0101-0101-0101-010101010101");
        assert!(uid(&canonical).is_some());
        assert!(
            uid(&VirtualEndpointId::from(
                "ABCDEF01-0101-0101-0101-010101010101"
            ))
            .is_none()
        );
        assert!(uid(&VirtualEndpointId::from("missing")).is_none());
    }
    #[test]
    fn inactive_capability_does_not_claim_actual_io_and_bounds_respect_callback_capacity() {
        let info = BufferInfo {
            device: 9,
            frames: 512,
            minimum: 15.0,
            maximum: 8192.0,
            settable: 1,
        };
        let inactive = settings(&info, false, Some(128)).unwrap();
        assert_eq!(inactive.current_frames, None);
        assert_eq!(inactive.preferred_frames, Some(128));
        assert_eq!(inactive.maximum_frames, 4096);
        assert!(!inactive.accepts(8192));
        assert_eq!(
            settings(&info, true, None).unwrap().current_frames,
            Some(512)
        );
        assert!(
            settings(
                &BufferInfo {
                    minimum: f64::NAN,
                    ..info
                },
                true,
                None
            )
            .is_none()
        );
    }
}

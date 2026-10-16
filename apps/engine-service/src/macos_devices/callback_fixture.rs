//! In-process native owners served by the production callback bridge, without `CoreAudio`.
//! Route tests drive their `IOProc` with explicit host timestamps.
use super::{
    DeviceSet, MacDevices,
    device::Device,
    direct::{Binding, DirectPlan},
    spec::Spec,
    tests::synthetic_device,
    worker::callback::CallbackBridge,
};
use mixrelay_audio_graph::DeviceSelector;
use mixrelay_macos_transport::{audio_buffer::AudioBuffer, device_io::mr_device_io_process};
use std::{
    collections::BTreeMap,
    ptr,
    sync::{Arc, atomic::Ordering},
};

/// `CoreAudio`'s `AudioTimeStamp`, the layout the native callback reads.
#[repr(C)]
#[derive(Default)]
struct Timestamp {
    sample: f64,
    host: u64,
    rate_scalar: f64,
    word_clock: u64,
    smpte: [u8; 24],
    flags: u32,
    reserved: u32,
}
const _: () = assert!(size_of::<Timestamp>() == 64 && std::mem::offset_of!(Timestamp, flags) == 56);

#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

/// One callback's native timing. Host times are mach ticks.
#[derive(Clone, Copy)]
pub(crate) struct CallbackTime {
    pub(crate) now: u64,
    /// Capture time of the first input frame, or presentation time of the first output frame.
    pub(crate) host: u64,
    pub(crate) sample: f64,
    pub(crate) rate_scalar: f64,
}

pub(crate) struct NativeOwner {
    device: Arc<Device>,
    context: super::worker::WorkerContext,
}

impl NativeOwner {
    pub(crate) fn new(channels: u16, rate: u32, input: bool) -> Self {
        let (device, context) = synthetic_device(channels, rate, rate, input);
        Self { device, context }
    }

    /// Publishes the converter the connected device worker would, at the current clock alignment.
    #[must_use]
    pub(crate) fn serve(&self) -> impl Sized + '_ {
        CallbackBridge::new(
            &self.context,
            self.device.native_rate.load(Ordering::Acquire),
        )
        .expect("synthetic callback bridge")
    }

    /// One `IOProc` invocation over interleaved PCM: read for input, written for output.
    pub(crate) fn callback(&self, pcm: &mut [f32], time: CallbackTime) {
        let mut list = BufferList {
            count: 1,
            buffers: [AudioBuffer {
                channels: u32::try_from(self.device.channels).expect("channel count"),
                byte_size: u32::try_from(size_of_val(pcm)).expect("buffer size"),
                data: pcm.as_mut_ptr(),
            }],
        };
        let now = Timestamp {
            host: time.now,
            flags: 2,
            ..Timestamp::default()
        };
        // Sample, host and rate scalar are valid, as `CoreAudio` supplies them.
        let stream = Timestamp {
            sample: time.sample,
            host: time.host,
            rate_scalar: time.rate_scalar,
            flags: 7,
            ..Timestamp::default()
        };
        let input = self.device.input;
        // SAFETY: no OS registration exists, so this fixture is the owner's only serialized
        // callback. The list covers exactly `pcm`, and both timestamps outlive the call.
        unsafe {
            mr_device_io_process(
                0,
                ptr::from_ref(&now).cast(),
                if input {
                    ptr::from_ref(&list).cast()
                } else {
                    ptr::null()
                },
                if input {
                    ptr::from_ref(&stream).cast()
                } else {
                    ptr::null()
                },
                if input {
                    ptr::null_mut()
                } else {
                    ptr::from_mut(&mut list).cast()
                },
                if input {
                    ptr::null()
                } else {
                    ptr::from_ref(&stream).cast()
                },
                self.context.native.pointer.as_ptr(),
            );
        }
    }
}

/// The active set and the reporter's cache, keyed by graph binding id as preparation keys them.
pub(crate) fn publish(devices: &MacDevices, owners: &[(&str, &NativeOwner)]) -> DeviceSet {
    let mut cache = devices.cache.lock().expect("device cache");
    let mut active = BTreeMap::new();
    for (id, owner) in owners {
        let spec = Spec {
            selector: DeviceSelector::PersistentUid {
                uid: "synthetic-no-device".into(),
            },
            input: owner.device.input,
            channels: u16::try_from(owner.device.channels).expect("channel count"),
        };
        cache.insert((*id).into(), (spec, Arc::downgrade(&owner.device)));
        active.insert((*id).into(), Arc::clone(&owner.device));
    }
    DeviceSet(active, BTreeMap::new())
}

/// A direct plan whose `master` callback renders its one graph binding `node` (keyed
/// `key` in the device set) in place, while `bridged` devices keep their converters.
pub(crate) fn direct_plan(
    node: &str,
    key: &str,
    master: &NativeOwner,
    bridged: DeviceSet,
) -> DirectPlan {
    let device = &master.device;
    DirectPlan {
        master: Arc::clone(device),
        owners: vec![Arc::clone(device)],
        latency_bindings: vec![(key.into(), Arc::clone(device))],
        bindings: vec![Binding {
            id: node.into(),
            input: device.input,
            offset: Some(0),
            channels: device.channels,
        }],
        bridged,
        clock_id: 1,
    }
}

//! A recovery aggregate and its Rust audio test share one process. A private
//! device therefore suffices: no test device is published to other applications.
use core_foundation::{
    array::CFArray,
    base::TCFType,
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const ENDPOINT_UID: &str = "io.tonenari.audio.endpoint.default";
const PREFIX: &str = "io.tonenari.device-recovery-probe.";
const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const BAD_OBJECT: i32 = i32::from_be_bytes(*b"!obj");
const BAD_SIZE: i32 = i32::from_be_bytes(*b"!siz");
const TIMEOUT: Duration = Duration::from_secs(3);
const INTERVAL: Duration = Duration::from_millis(20);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}
#[repr(C)]
#[derive(Default)]
struct StreamFormat {
    sample_rate: f64,
    format_id: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioHardwareCreateAggregateDevice(description: *const c_void, device: *mut u32) -> i32;
    fn AudioHardwareDestroyAggregateDevice(device: u32) -> i32;
    fn AudioObjectGetPropertyData(
        device: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

fn description(uid: &str) -> CFDictionary<CFString, core_foundation::base::CFType> {
    let member =
        CFDictionary::from_CFType_pairs(&[(CFString::new("uid"), CFString::new(ENDPOINT_UID))]);
    let members = CFArray::from_CFTypes(&[member]);
    CFDictionary::from_CFType_pairs(&[
        (CFString::new("uid"), CFString::new(uid).as_CFType()),
        (CFString::new("name"), CFString::new(uid).as_CFType()),
        (CFString::new("subdevices"), members.as_CFType()),
        (
            CFString::new("private"),
            CFBoolean::true_value().as_CFType(),
        ),
    ])
}

fn read<T>(device: u32, selector: u32, scope: u32, value: &mut T) -> Result<(), i32> {
    let address = Address {
        selector,
        scope,
        element: 0,
    };
    let mut size = u32::try_from(size_of::<T>()).expect("bounded native property");
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &raw const address,
            0,
            ptr::null(),
            &raw mut size,
            ptr::from_mut(value).cast(),
        )
    };
    if status != 0 {
        Err(status)
    } else if size as usize != size_of::<T>() {
        Err(BAD_SIZE)
    } else {
        Ok(())
    }
}

fn device_uid(device: u32) -> Result<String, i32> {
    let mut value: CFStringRef = ptr::null();
    let valid = read(device, u32::from_be_bytes(*b"uid "), GLOBAL, &mut value);
    if value.is_null() {
        return Err(valid.err().unwrap_or(BAD_SIZE));
    }
    // CoreAudio transfers a retained CFString. Acquire its owner before checking
    // status so even an unexpected partial property result is released.
    let value = unsafe { CFString::wrap_under_create_rule(value) };
    valid.map(|()| value.to_string())
}

fn stream_ready(device: u32, scope: u32) -> bool {
    let address = Address {
        selector: u32::from_be_bytes(*b"stm#"),
        scope,
        element: 0,
    };
    let mut streams = [0_u32; 64];
    let mut bytes = u32::try_from(size_of_val(&streams)).expect("bounded stream array");
    if unsafe {
        AudioObjectGetPropertyData(
            device,
            &raw const address,
            0,
            ptr::null(),
            &raw mut bytes,
            streams.as_mut_ptr().cast(),
        )
    } != 0
        || bytes == 0
        || bytes as usize > size_of_val(&streams)
        || bytes % 4 != 0
    {
        return false;
    }
    let mut channels = 0_u64;
    for &stream in &streams[..bytes as usize / 4] {
        let mut format = StreamFormat::default();
        if read(stream, u32::from_be_bytes(*b"sfmt"), GLOBAL, &mut format).is_err()
            || !format.sample_rate.is_finite()
            || format.sample_rate <= 0.0
            || format.format_id != u32::from_be_bytes(*b"lpcm")
            || format.bits_per_channel != 32
        {
            return false;
        }
        channels += u64::from(format.channels);
    }
    channels >= 2
}

pub(super) struct PrivateDevice {
    uid: String,
    device: Option<u32>,
}

impl PrivateDevice {
    pub(super) fn new() -> Result<Self, String> {
        let uid = format!(
            "{PREFIX}{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let mut owned = Self { uid, device: None };
        owned.connect()?;
        Ok(owned)
    }
    pub(super) fn uid(&self) -> &str {
        &self.uid
    }

    pub(super) fn connect(&mut self) -> Result<(), String> {
        if self.device.is_some() {
            return Err("private fixture is already connected".into());
        }
        let description = description(&self.uid);
        let mut device = 0;
        let status = unsafe {
            AudioHardwareCreateAggregateDevice(
                description.as_concrete_TypeRef().cast(),
                &raw mut device,
            )
        };
        // A nonzero object ID transfers ownership even when creation reports an error.
        self.device = (device != 0).then_some(device);
        if status != 0 || device == 0 {
            return Err(format!(
                "private aggregate creation failed: status={status}, device={device}"
            ));
        }
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if device_uid(device).as_deref() == Ok(self.uid.as_str())
                && stream_ready(device, u32::from_be_bytes(*b"inpt"))
                && stream_ready(device, u32::from_be_bytes(*b"outp"))
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "private aggregate streams did not become ready: {}",
                    self.uid
                ));
            }
            thread::sleep(INTERVAL);
        }
    }

    pub(super) fn disconnect(&mut self) -> Result<(), String> {
        let Some(device) = self.device else {
            return Err("private fixture is already disconnected".into());
        };
        let deadline = Instant::now() + TIMEOUT;
        let mut destroyed = false;
        loop {
            let status = match device_uid(device) {
                Err(BAD_OBJECT) => {
                    self.device = None;
                    return Ok(());
                }
                Ok(uid) if uid != self.uid => {
                    if destroyed {
                        self.device = None;
                        return Ok(());
                    }
                    return Err("private fixture object ID now belongs to a different device; refusing destruction".into());
                }
                Ok(_) if destroyed => 0,
                Ok(_) => {
                    let status = unsafe { AudioHardwareDestroyAggregateDevice(device) };
                    destroyed = status == 0;
                    status
                }
                // A transient property failure does not prove destruction.
                // Retain ownership, retry, and fail cleanup visibly on timeout.
                Err(status) => status,
            };
            if Instant::now() >= deadline {
                return Err(format!(
                    "private aggregate destruction failed: {} status={status}",
                    self.uid
                ));
            }
            thread::sleep(INTERVAL);
        }
    }
}

impl Drop for PrivateDevice {
    fn drop(&mut self) {
        if self.device.is_some()
            && let Err(error) = self.disconnect()
        {
            eprintln!("{error}");
            assert!(
                thread::panicking(),
                "private fixture cleanup failed: {error}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_description_is_private_and_contains_only_the_tonenari_endpoint() {
        let description = description("io.tonenari.device-recovery-probe.contract");
        assert_eq!(description.len(), 4);
        let private = description
            .get(CFString::new("private"))
            .downcast::<CFBoolean>()
            .unwrap();
        assert_eq!(private, CFBoolean::true_value());
        let subs = description
            .get(CFString::new("subdevices"))
            .downcast::<CFArray>()
            .unwrap();
        assert_eq!(subs.len(), 1);
    }

    #[test]
    fn native_private_fixture_is_invisible_to_other_process_and_destroyed_on_unwind() {
        let mut owned = PrivateDevice::new().unwrap();
        let id = owned.device.unwrap();
        assert_eq!(device_uid(id).as_deref(), Ok(owned.uid()));
        let inventory = crate::process::run(
            std::process::Command::new(crate::device_fixture()).arg("--inventory"),
            Duration::from_secs(6),
        )
        .unwrap();
        assert!(inventory.status.success());
        assert!(!String::from_utf8_lossy(&inventory.stdout).contains(owned.uid()));
        owned.disconnect().unwrap();
        assert_eq!(device_uid(id), Err(BAD_OBJECT));
        owned.connect().unwrap();
        let id = owned.device.unwrap();
        let failure = std::panic::catch_unwind(move || {
            let _owned = owned;
            panic!("exercise owned private fixture unwind");
        });
        assert!(failure.is_err());
        assert_eq!(device_uid(id), Err(BAD_OBJECT));
    }
}

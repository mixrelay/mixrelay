use std::{collections::BTreeMap, mem::size_of};

use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::{CloseHandle, HANDLE},
        Media::Audio::{
            DEVICE_STATE_ACTIVE, EDataFlow, IAudioClient, IMMDevice, IMMDeviceEnumerator,
            MMDeviceEnumerator, eCapture, eConsole, eRender,
        },
        System::{
            Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                CoUninitialize, STGM_READ, StructuredStorage::PropVariantToStringAlloc,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
        },
    },
    core::PWSTR,
};

use crate::{
    ApplicationSelector, AudioDeviceDescriptor, CapturableApplication, HostAudioBackendError,
};

#[path = "windows_application_metadata.rs"]
mod application_metadata;
#[path = "windows_process_identity.rs"]
pub mod process_identity;

/// Returns current processes addressable by Windows process-loopback capture.
///
/// New selections retain exact executable intent; protected processes can only
/// expose an explicit legacy PID selector. Localized captions remain display-only.
///
/// # Errors
///
/// Returns a backend error when the Windows process snapshot cannot be created.
pub fn applications() -> Result<Vec<CapturableApplication>, HostAudioBackendError> {
    // SAFETY: this function owns every native handle it opens.
    unsafe { applications_inner() }.map_err(backend_failure)
}

unsafe fn applications_inner() -> windows::core::Result<Vec<CapturableApplication>> {
    // SAFETY: the snapshot handle is uniquely owned and `dwSize` is initialized before use.
    unsafe {
        // Shell icon lookup requires COM; inventory remains on this apartment.
        let _apartment = initialize_com()?;
        let snapshot = OwnedHandle::new(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).expect("entry size fits u32"),
            ..Default::default()
        };
        let mut applications = Vec::new();
        if Process32FirstW(snapshot.get(), &raw mut entry).is_ok() {
            loop {
                let name = wide_array_to_string(&entry.szExeFile);
                if entry.th32ProcessID != 0 && !name.is_empty() {
                    let metadata = application_metadata::for_process(entry.th32ProcessID);
                    applications.push(CapturableApplication {
                        bundle_id: format!("process:{}", entry.th32ProcessID),
                        display_name: metadata
                            .display_name
                            .unwrap_or_else(|| strip_executable_suffix(&name).to_owned()),
                        icon_data_url: metadata.icon_data_url,
                        process_id: entry.th32ProcessID,
                        selector: process_identity::ProcessIdentity::open(entry.th32ProcessID)
                            .map_or(
                                ApplicationSelector::ProcessId {
                                    process_id: entry.th32ProcessID,
                                    include_descendants: true,
                                },
                                |identity| ApplicationSelector::ExecutablePath {
                                    path: identity.path().to_owned(),
                                },
                            ),
                    });
                }
                if Process32NextW(snapshot.get(), &raw mut entry).is_err() {
                    break;
                }
            }
        }
        applications.sort_unstable_by(|left, right| {
            left.display_name
                .cmp(&right.display_name)
                .then(left.process_id.cmp(&right.process_id))
        });
        let mut executable_intents = std::collections::BTreeSet::new();
        applications.retain(|application| match &application.selector {
            ApplicationSelector::ExecutablePath { path } => executable_intents.insert(path.clone()),
            _ => true,
        });
        Ok(applications)
    }
}

/// Returns active WASAPI render and capture endpoints and their current mix channel counts.
///
/// # Errors
///
/// Returns a backend error when COM or the multimedia device registry is unavailable.
pub fn devices() -> Result<Vec<AudioDeviceDescriptor>, HostAudioBackendError> {
    // SAFETY: this function keeps COM interfaces on the initialized calling thread.
    unsafe { devices_inner() }.map_err(backend_failure)
}

unsafe fn devices_inner() -> windows::core::Result<Vec<AudioDeviceDescriptor>> {
    // SAFETY: every COM object is created and released on this initialized thread.
    unsafe {
        let _apartment = initialize_com()?;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let default_input = default_endpoint_id(&enumerator, eCapture);
        let default_output = default_endpoint_id(&enumerator, eRender);
        let mut devices = BTreeMap::new();
        append_endpoints(
            &enumerator,
            eCapture,
            default_input.as_deref(),
            true,
            &mut devices,
        )?;
        append_endpoints(
            &enumerator,
            eRender,
            default_output.as_deref(),
            false,
            &mut devices,
        )?;
        let mut devices = devices.into_values().collect::<Vec<_>>();
        devices.sort_unstable_by(|left, right| {
            left.display_name
                .cmp(&right.display_name)
                .then(left.uid.cmp(&right.uid))
        });
        Ok(devices)
    }
}

unsafe fn append_endpoints(
    enumerator: &IMMDeviceEnumerator,
    flow: EDataFlow,
    default_id: Option<&str>,
    input: bool,
    devices: &mut BTreeMap<String, AudioDeviceDescriptor>,
) -> windows::core::Result<()> {
    // SAFETY: caller initialized COM and retains the enumerator through the loop.
    let collection = unsafe { enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)? };
    let count = unsafe { collection.GetCount()? };
    for index in 0..count {
        let device = unsafe { collection.Item(index)? };
        let uid = unsafe { endpoint_id(&device)? };
        let channels = unsafe { endpoint_channels(&device)? };
        let display_name =
            unsafe { endpoint_display_name(&device) }.unwrap_or_else(|_| uid.clone());
        let descriptor = devices
            .entry(uid.clone())
            .or_insert_with(|| AudioDeviceDescriptor {
                uid: uid.clone(),
                display_name,
                input_channels: 0,
                output_channels: 0,
                is_default_input: false,
                is_default_output: false,
            });
        if input {
            descriptor.input_channels = descriptor.input_channels.max(channels);
            descriptor.is_default_input |= default_id == Some(uid.as_str());
        } else {
            descriptor.output_channels = descriptor.output_channels.max(channels);
            descriptor.is_default_output |= default_id == Some(uid.as_str());
        }
    }
    Ok(())
}

unsafe fn endpoint_id(device: &IMMDevice) -> windows::core::Result<String> {
    let value = unsafe { device.GetId()? };
    let result = String::from_utf16_lossy(unsafe { value.as_wide() });
    unsafe { CoTaskMemFree(Some(value.as_ptr().cast())) };
    Ok(result)
}

unsafe fn endpoint_display_name(device: &IMMDevice) -> windows::core::Result<String> {
    let store = unsafe { device.OpenPropertyStore(STGM_READ)? };
    let key = PKEY_Device_FriendlyName;
    let value = unsafe { store.GetValue(&raw const key)? };
    // windows-rs owns and clears the returned PROPVARIANT, including errors.
    let text: PWSTR = unsafe { PropVariantToStringAlloc(&raw const value)? };
    let result = String::from_utf16_lossy(unsafe { text.as_wide() });
    unsafe { CoTaskMemFree(Some(text.as_ptr().cast())) };
    Ok(result)
}

unsafe fn endpoint_channels(device: &IMMDevice) -> windows::core::Result<u32> {
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let format = unsafe { client.GetMixFormat()? };
    let channels = u32::from(unsafe { (*format).nChannels });
    unsafe { CoTaskMemFree(Some(format.cast())) };
    Ok(channels)
}

unsafe fn default_endpoint_id(enumerator: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    unsafe {
        enumerator
            .GetDefaultAudioEndpoint(flow, eConsole)
            .and_then(|device| endpoint_id(&device))
            .ok()
    }
}

struct ComInitialization(bool);
impl Drop for ComInitialization {
    fn drop(&mut self) {
        if self.0 {
            // Balance only our successful initialization, after local interfaces drop.
            unsafe { CoUninitialize() };
        }
    }
}

unsafe fn initialize_com() -> windows::core::Result<ComInitialization> {
    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result.is_ok() {
        Ok(ComInitialization(true))
    } else if result.0 == 0x8001_0106_u32.cast_signed() {
        // Inventory can run in the caller's existing STA. We did not initialize
        // that apartment, so must not decrement its reference count.
        Ok(ComInitialization(false))
    } else {
        Err(result.into())
    }
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(handle: HANDLE) -> windows::core::Result<Self> {
        if handle.is_invalid() {
            Err(windows::core::Error::from_win32())
        } else {
            Ok(Self(handle))
        }
    }

    const fn get(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns its snapshot or process handle exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn wide_array_to_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

fn strip_executable_suffix(value: &str) -> &str {
    value
        .strip_suffix(".exe")
        .or_else(|| value.strip_suffix(".EXE"))
        .unwrap_or(value)
}

fn backend_failure(error: impl std::fmt::Display) -> HostAudioBackendError {
    HostAudioBackendError::BackendFailure {
        message: error.to_string(),
    }
}

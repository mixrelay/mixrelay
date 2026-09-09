//! Opt-in destructive-boundary acceptance, restricted to the authorized LAB VM
//! and non-default `ToneNari` `AudioEndpoint` instances. Never used by the product.
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};
use tonenari_audio_io::AudioDeviceDescriptor;

pub(super) fn endpoint(input: bool) -> AudioDeviceDescriptor {
    assert_eq!(std::env::var("TONENARI_LAB_PNP_TEST").as_deref(), Ok("1"));
    let devices = tonenari_audio_io::windows::devices().unwrap();
    if std::env::var("TONENARI_LAB_PNP_SCOPE").as_deref() == Ok("RootLab") {
        assert_defaults_outside_lab(&devices);
    }
    let result = devices
        .into_iter()
        .find(|device| {
            device.display_name.contains("ToneNari SysVAD Bridge LAB")
                && !device.is_default_input
                && !device.is_default_output
                && if input {
                    device.input_channels > 0
                } else {
                    device.output_channels > 0
                }
        })
        .expect("a non-default installed ToneNari LAB endpoint");
    eprintln!("PnP target UID={} name={}", result.uid, result.display_name);
    result
}

fn toggle(uid: &str, action: &str) -> Result<(), String> {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/lab/windows-lab-endpoint-toggle.ps1");
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .args(["-Uid", uid, "-Action", action, "-Scope"])
        .arg(std::env::var("TONENARI_LAB_PNP_SCOPE").unwrap_or_else(|_| "Endpoint".into()))
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{action}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn assert_defaults_outside_lab(devices: &[AudioDeviceDescriptor]) {
    let has_lab_default = default_roles().iter().any(|(_, _, uid)| {
        devices.iter().any(|device| {
            Some(&device.uid) == uid.as_ref() && device.display_name.contains("ToneNari")
        })
    });
    // Explicit additional user authorization is required if the exact LAB root
    // currently serves an OS default. No test changes a default selection.
    assert!(
        !has_lab_default
            || std::env::var("TONENARI_LAB_ALLOW_DEFAULT_DRIVER_STOP").as_deref() == Ok("1"),
        "refusing to stop a LAB device used as an OS default"
    );
}

fn default_roles() -> Vec<(i32, i32, Option<String>)> {
    use windows::Win32::{
        Media::Audio::{
            IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications, eConsole,
            eMultimedia, eRender,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree},
    };
    let _apartment = super::render::ComApartment::new().unwrap();
    // Every role is checked, not only the console role exposed by discovery.
    let mut result = Vec::new();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
        for flow in [eCapture, eRender] {
            for role in [eConsole, eMultimedia, eCommunications] {
                let uid = if let Ok(device) = enumerator.GetDefaultAudioEndpoint(flow, role) {
                    let id = device.GetId().unwrap();
                    let uid = String::from_utf16_lossy(id.as_wide());
                    CoTaskMemFree(Some(id.as_ptr().cast()));
                    Some(uid)
                } else {
                    None
                };
                result.push((flow.0, role.0, uid));
            }
        }
    }
    eprintln!(
        "DEFAULT_ENDPOINTS_JSON={}",
        serde_json::to_string(&result).unwrap()
    );
    result
}

#[test]
#[ignore = "read-only inventory for authorized interactive Windows VM acceptance"]
fn native_wasapi_default_roles_read_only() {
    assert_eq!(std::env::var("TONENARI_LAB_PNP_TEST").as_deref(), Ok("1"));
    assert_eq!(default_roles().len(), 6);
}

/// The guard is armed before disabling, so even partially successful native
/// operations unwind through re-enable. The outer scheduled probe also restores.
pub(super) struct DisabledEndpoint {
    uid: String,
    armed: bool,
}
impl DisabledEndpoint {
    pub(super) fn new(device: &AudioDeviceDescriptor) -> Self {
        let guard = Self {
            uid: device.uid.clone(),
            armed: true,
        };
        toggle(&guard.uid, "Disable").unwrap();
        guard
    }
    pub(super) fn restore(&mut self) {
        toggle(&self.uid, "Enable").unwrap();
        self.armed = false;
    }
}
impl Drop for DisabledEndpoint {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = toggle(&self.uid, "Enable")
        {
            eprintln!("CRITICAL: LAB endpoint restore failed: {error}");
        }
    }
}

pub(super) fn wait_until(label: &str, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timeout: {label}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

//! Which apps are playing into or recording from each `MixRelay` virtual device, from
//! `CoreAudio` process objects. Helper processes (a browser's audio helper) are shown as
//! the app they belong to, and `MixRelay`'s own processes (the engine) are left out.
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, PoisonError},
};

use mixrelay_audio_io::CapturableApplication;
use mixrelay_process_tap::DeviceUse;
use serde::Serialize;

/// `MixRelay` virtual devices are named by this UID prefix.
const ENDPOINT_PREFIX: &str = "io.mixrelay.audio.endpoint.";
/// `MixRelay`'s own processes use every device; they are not "apps using" it.
const OWN_PREFIX: &str = "io.mixrelay.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceAppActivity {
    pub(crate) bundle_id: String,
    pub(crate) display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon_data_url: Option<String>,
    pub(crate) process_id: i32,
    pub(crate) playing: bool,
    pub(crate) recording: bool,
}

/// Display name and icon of a running app.
type Identity = (String, Option<String>);
/// Running apps by bundle ID.
type Identities = HashMap<String, Identity>;

/// Identities are fetched from the system only when an unknown bundle appears, so the
/// once-a-second reads do not re-encode every app icon.
static IDENTITIES: Mutex<Option<Identities>> = Mutex::new(None);

fn identities_from(applications: Vec<CapturableApplication>) -> Identities {
    applications
        .into_iter()
        .map(|app| (app.bundle_id, (app.display_name, app.icon_data_url)))
        .collect()
}

/// The running app a bundle belongs to: itself, or the app whose bundle ID prefixes it
/// (`com.google.Chrome.helper` belongs to `com.google.Chrome`), longest match first.
fn owner<'a>(bundle_id: &str, identities: &'a Identities) -> Option<(&'a str, &'a Identity)> {
    identities
        .iter()
        .filter(|(app, _)| {
            bundle_id == app.as_str()
                || bundle_id
                    .strip_prefix(app.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
        })
        .max_by_key(|(app, _)| app.len())
        .map(|(app, identity)| (app.as_str(), identity))
}

/// Groups device uses by `MixRelay` device, one entry per app (helpers merged into their
/// app, flags combined), in a stable order.
pub(crate) fn group(
    uses: Vec<DeviceUse>,
    identities: &Identities,
) -> BTreeMap<String, Vec<DeviceAppActivity>> {
    let mut devices: BTreeMap<String, Vec<DeviceAppActivity>> = BTreeMap::new();
    for entry in uses {
        if !entry.device_uid.starts_with(ENDPOINT_PREFIX) || entry.bundle_id.starts_with(OWN_PREFIX)
        {
            continue;
        }
        let (bundle_id, display_name, icon_data_url) = match owner(&entry.bundle_id, identities) {
            Some((app, (name, icon))) => (app.to_owned(), name.clone(), icon.clone()),
            None => (entry.bundle_id.clone(), entry.bundle_id.clone(), None),
        };
        let apps = devices.entry(entry.device_uid).or_default();
        match apps.iter_mut().find(|app| app.bundle_id == bundle_id) {
            Some(app) => {
                app.playing |= entry.playing;
                app.recording |= entry.recording;
            }
            None => apps.push(DeviceAppActivity {
                bundle_id,
                display_name,
                icon_data_url,
                process_id: entry.process_id,
                playing: entry.playing,
                recording: entry.recording,
            }),
        }
    }
    for apps in devices.values_mut() {
        apps.sort_by(|left, right| {
            left.display_name
                .cmp(&right.display_name)
                .then(left.bundle_id.cmp(&right.bundle_id))
        });
    }
    devices
}

/// Apps currently using each `MixRelay` device, keyed by its native UID.
pub(super) fn activity() -> Result<BTreeMap<String, Vec<DeviceAppActivity>>, String> {
    let uses = mixrelay_process_tap::device_uses().map_err(|error| error.to_string())?;
    let mut cached = IDENTITIES.lock().unwrap_or_else(PoisonError::into_inner);
    let unknown = uses.iter().any(|entry| {
        !entry.bundle_id.starts_with(OWN_PREFIX)
            && cached
                .as_ref()
                .is_none_or(|identities| owner(&entry.bundle_id, identities).is_none())
    });
    if unknown {
        *cached = Some(identities_from(super::list_capturable_applications()?));
    }
    Ok(group(uses, cached.as_ref().unwrap_or(&Identities::new())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device_use(bundle_id: &str, device: &str, playing: bool, recording: bool) -> DeviceUse {
        DeviceUse {
            process_id: 7,
            bundle_id: bundle_id.to_owned(),
            device_uid: format!("{ENDPOINT_PREFIX}{device}"),
            playing,
            recording,
        }
    }

    #[test]
    fn helpers_join_their_app_and_mixrelay_processes_are_left_out() {
        let identities: Identities = [
            (
                "com.google.Chrome".to_owned(),
                ("Chrome".to_owned(), Some("icon".to_owned())),
            ),
            ("com.google".to_owned(), ("Too broad".to_owned(), None)),
            ("com.apple.Music".to_owned(), ("Music".to_owned(), None)),
        ]
        .into_iter()
        .collect();
        let uses = vec![
            device_use("com.google.Chrome.helper", "a", true, false),
            device_use("com.google.Chrome", "a", false, true),
            device_use("com.apple.Music", "a", true, false),
            device_use("io.mixrelay.audio.engine", "a", true, true),
            device_use("org.unknown.Tool", "b", false, true),
            DeviceUse {
                device_uid: "BuiltInSpeakerDevice".to_owned(),
                ..device_use("com.apple.Music", "", true, false)
            },
        ];
        let devices = group(uses, &identities);
        let app =
            |bundle: &str, name: &str, icon: Option<&str>, playing, recording| DeviceAppActivity {
                bundle_id: bundle.to_owned(),
                display_name: name.to_owned(),
                icon_data_url: icon.map(str::to_owned),
                process_id: 7,
                playing,
                recording,
            };
        assert_eq!(
            devices[&format!("{ENDPOINT_PREFIX}a")],
            [
                app("com.google.Chrome", "Chrome", Some("icon"), true, true),
                app("com.apple.Music", "Music", None, true, false),
            ]
        );
        assert_eq!(
            devices[&format!("{ENDPOINT_PREFIX}b")],
            [app(
                "org.unknown.Tool",
                "org.unknown.Tool",
                None,
                false,
                true
            )]
        );
        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn the_live_system_answers() {
        assert!(activity().is_ok());
    }
}

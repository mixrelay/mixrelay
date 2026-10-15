//! Device buffer choices use the runtime's native I/O readback.
use std::collections::HashMap;

use mixrelay_audio_io::{NativeBufferSettings, VirtualEndpointDeviceSettings, VirtualEndpointId};
use mixrelay_control_protocol::{GraphRuntimeStatus, VirtualEndpointBufferRequest};
use tauri::{
    Manager as _,
    menu::{CheckMenuItem, Submenu},
};

use super::{MenuState, StatusMenu, menu::Labels};

// Native callback capacity is independent of the smaller DSP slice catalog.
const NATIVE_BUFFER_PRESETS: [u32; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BufferAction {
    endpoint_id: VirtualEndpointId,
    driver_revision: u64,
    expected_frames: Option<u32>,
    expected_preferred_frames: Option<u32>,
    frames: u32,
}

pub(super) fn observed_buffers(
    session: Option<mixrelay_audio_io::session::AudioSessionStatus>,
) -> HashMap<VirtualEndpointId, NativeBufferSettings> {
    session
        .and_then(|session| session.virtual_endpoint_settings)
        .into_iter()
        .flat_map(|settings| settings.devices)
        .filter_map(|device| device.native_buffer.map(|buffer| (device.spec.id, buffer)))
        .collect()
}

fn options(buffer: NativeBufferSettings) -> Vec<u32> {
    let mut frames: Vec<_> = NATIVE_BUFFER_PRESETS
        .into_iter()
        .filter(|frames| buffer.accepts(*frames))
        .collect();
    for value in [buffer.current_frames, buffer.preferred_frames]
        .into_iter()
        .flatten()
    {
        if value != 0 {
            frames.push(value);
        }
    }
    frames.sort_unstable();
    frames.dedup();
    frames
}

fn can_select(buffer: NativeBufferSettings, frames: u32) -> bool {
    buffer.can_set && NATIVE_BUFFER_PRESETS.contains(&frames) && buffer.accepts(frames)
}

fn value_label(frames: u32, saved: bool, text: &Labels) -> String {
    if saved {
        format!("{frames} ({})", text.saved)
    } else {
        frames.to_string()
    }
}

pub(super) fn append_menu(
    app: &tauri::AppHandle,
    state: &mut MenuState,
    device: &VirtualEndpointDeviceSettings,
    parent: &Submenu<tauri::Wry>,
    text: &Labels,
) -> tauri::Result<()> {
    let Some(revision) = state.snapshot.as_ref().map(|snapshot| snapshot.revision) else {
        return Ok(());
    };
    let Some(buffer) = state.buffers.get(&device.spec.id).copied() else {
        return Ok(());
    };
    let value = buffer.current_frames.or(buffer.preferred_frames);
    let name = value.map_or_else(
        || text.buffer_size.to_owned(),
        |frames| {
            format!(
                "{} · {}",
                text.buffer_size,
                value_label(frames, buffer.current_frames.is_none(), text)
            )
        },
    );
    let enabled = buffer.can_set
        && !state.audio_power.is_some_and(|power| power.transitioning)
        && options(buffer)
            .into_iter()
            .any(|frames| can_select(buffer, frames));
    let menu = Submenu::new(app, name, enabled)?;
    for frames in options(buffer) {
        let id = format!(
            "status:buffer:{}:{}:{frames}",
            state.generation,
            device.spec.id.as_str()
        );
        let selectable = can_select(buffer, frames);
        if selectable {
            state.buffer_actions.insert(
                id.clone(),
                BufferAction {
                    endpoint_id: device.spec.id.clone(),
                    driver_revision: revision,
                    expected_frames: buffer.current_frames,
                    expected_preferred_frames: buffer.preferred_frames,
                    frames,
                },
            );
        }
        let saved =
            buffer.preferred_frames == Some(frames) && buffer.current_frames != Some(frames);
        menu.append(&CheckMenuItem::with_id(
            app,
            id,
            value_label(frames, saved, text),
            selectable,
            buffer.current_frames == Some(frames),
            None::<&str>,
        )?)?;
    }
    parent.append(&menu)
}

pub(super) fn select(app: &tauri::AppHandle, id: &str) {
    let menu = app.state::<StatusMenu>();
    let Ok(mut state) = menu.state.lock() else {
        return;
    };
    if state.busy
        || state.audio_change.is_some()
        || state.audio_power.is_some_and(|power| power.transitioning)
        || state.stopping
    {
        return;
    }
    let Some(action) = state.buffer_actions.get(id).cloned() else {
        return;
    };
    state.busy = true;
    state.error = None;
    let locale = state.locale.clone();
    drop(state);
    let _ = menu.refresh_menu();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = apply(&app, action).await;
        super::playback::refresh(&app).await;
        let snapshot = crate::driver_control::driver_status(app.clone()).await;
        super::finish_snapshot(&app, snapshot);
        if let Err(error) = result {
            tracing::error!(%error, "Driver native buffer change failed");
            rfd::AsyncMessageDialog::new()
                .set_title("MixRelay")
                .set_description(super::menu::labels(&locale).buffer_failed)
                .set_buttons(rfd::MessageButtons::OkCustom(
                    crate::native_text::dialogs::text(
                        crate::native_text::resolve_locale(&locale),
                        crate::native_text::dialogs::Text::Ok,
                    )
                    .into(),
                ))
                .set_level(rfd::MessageLevel::Error)
                .show()
                .await;
        }
    });
}

fn request(
    action: &BufferAction,
    actual: NativeBufferSettings,
    graph: GraphRuntimeStatus,
) -> Result<VirtualEndpointBufferRequest, String> {
    if graph.active_revision != graph.submitted_revision {
        return Err("Audio graph change is pending; choose again".into());
    }
    if !can_select(actual, action.frames)
        || actual.current_frames != action.expected_frames
        || actual.preferred_frames != action.expected_preferred_frames
    {
        return Err("Device buffer settings changed; choose again".into());
    }
    Ok(VirtualEndpointBufferRequest {
        expected_revision: graph.active_revision,
        endpoint_id: action.endpoint_id.clone(),
        expected_buffer_frames: action.expected_frames,
        expected_preferred_frames: action.expected_preferred_frames,
        buffer_frames: action.frames,
    })
}

async fn apply(app: &tauri::AppHandle, action: BufferAction) -> Result<(), String> {
    let snapshot = crate::driver_control::driver_status(app.clone()).await?;
    if snapshot.state != mixrelay_driver_control::DriverState::Ready
        || snapshot.revision != action.driver_revision
        || !snapshot
            .settings
            .devices
            .iter()
            .any(|device| device.spec.id == action.endpoint_id)
    {
        return Err("Driver device settings changed; choose again".into());
    }
    let graph = crate::control_proxy::control_get_graph_status(app.state()).await?;
    if graph.status != 200 {
        return Err(graph.body.to_string());
    }
    let graph = serde_json::from_value(graph.body).map_err(|error| error.to_string())?;
    let status = crate::control_proxy::control_get_status(app.state(), app.clone()).await?;
    if status.status != 200 {
        return Err(status.body.to_string());
    }
    let session = serde_json::from_value(status.body["audioSession"].clone())
        .map_err(|error| error.to_string())?;
    let actual = observed_buffers(Some(session))
        .get(&action.endpoint_id)
        .copied()
        .ok_or("The device buffer is no longer available")?;
    let body = serde_json::to_value(request(&action, actual, graph)?)
        .map_err(|error| error.to_string())?;
    let response =
        crate::control_proxy::control_set_virtual_endpoint_buffer_frames(app.state(), body).await?;
    if response.status != 200 {
        return Err(response.body.to_string());
    }
    if let Some(error) = response.body["persistenceError"].as_str() {
        return Err(error.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer() -> NativeBufferSettings {
        NativeBufferSettings {
            current_frames: Some(128),
            preferred_frames: Some(256),
            minimum_frames: 64,
            maximum_frames: 4096,
            can_set: true,
        }
    }
    fn action() -> BufferAction {
        BufferAction {
            endpoint_id: VirtualEndpointId::from("device"),
            driver_revision: 12,
            expected_frames: Some(128),
            expected_preferred_frames: Some(256),
            frames: 256,
        }
    }
    fn graph() -> GraphRuntimeStatus {
        GraphRuntimeStatus {
            protocol_version: 1,
            active_revision: 4,
            submitted_revision: 4,
        }
    }
    #[test]
    fn presets_use_native_limits_and_keep_nonstandard_observations() {
        assert_eq!(options(buffer()), vec![64, 128, 256, 512, 1024, 2048, 4096]);
        let actual = NativeBufferSettings {
            current_frames: Some(15),
            preferred_frames: Some(4096),
            minimum_frames: 15,
            ..buffer()
        };
        assert_eq!(
            options(actual),
            vec![15, 32, 64, 128, 256, 512, 1024, 2048, 4096]
        );
        assert_eq!(
            options(NativeBufferSettings {
                can_set: false,
                ..actual
            }),
            vec![15, 4096]
        );
        assert!(!can_select(actual, 15));
        assert!(can_select(actual, 4096));
    }
    #[test]
    fn request_retains_target_and_fences_native_readback_and_graph_changes() {
        let request = request(&action(), buffer(), graph()).unwrap();
        assert_eq!(request.endpoint_id, VirtualEndpointId::from("device"));
        assert_eq!(request.expected_revision, 4);
        assert_eq!(request.expected_buffer_frames, Some(128));
        assert_eq!(request.expected_preferred_frames, Some(256));
        assert_eq!(request.buffer_frames, 256);
        for actual in [
            NativeBufferSettings {
                current_frames: Some(512),
                ..buffer()
            },
            NativeBufferSettings {
                can_set: false,
                ..buffer()
            },
            NativeBufferSettings {
                preferred_frames: Some(512),
                ..buffer()
            },
            NativeBufferSettings {
                maximum_frames: 128,
                ..buffer()
            },
        ] {
            assert!(super::request(&action(), actual, graph()).is_err());
        }
        assert!(
            super::request(
                &action(),
                buffer(),
                GraphRuntimeStatus {
                    submitted_revision: 5,
                    ..graph()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn saved_values_are_not_checked_or_reported_as_observed() {
        let mut action = action();
        action.expected_frames = None;
        let actual = NativeBufferSettings {
            current_frames: None,
            ..buffer()
        };
        assert_eq!(
            request(&action, actual, graph())
                .unwrap()
                .expected_buffer_frames,
            None
        );
        assert!(
            request(
                &action,
                NativeBufferSettings {
                    preferred_frames: Some(512),
                    ..actual
                },
                graph()
            )
            .is_err()
        );
        assert_eq!(
            value_label(256, true, &super::super::menu::labels("ko")),
            "256 (저장됨)"
        );
        assert_eq!(
            value_label(128, false, &super::super::menu::labels("ko")),
            "128"
        );
        assert!(observed_buffers(None).is_empty());
    }
}

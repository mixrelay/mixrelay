use mixrelay_audio_io::{
    ChannelLayout, NativeSampleFormat, VirtualEndpointDeviceSettings, VirtualEndpointId,
    VirtualEndpointSettings,
};
use mixrelay_driver_control::{DriverCommand, DriverSnapshot, DriverState};

/// One setting applied to every existing direction of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingChoice {
    Channels(u16),
    SampleRate(u32),
    SampleFormat(NativeSampleFormat),
}

/// The observed identity and revision attached to one menu choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SettingAction {
    pub(super) endpoint_id: VirtualEndpointId,
    pub(super) expected_revision: u64,
    pub(super) choice: SettingChoice,
}

/// Returns a channel count only when all existing directions agree.
pub(super) fn current_channels(device: &VirtualEndpointDeviceSettings) -> Option<u16> {
    match (device.spec.render, device.spec.capture) {
        (Some(render), Some(capture)) if render.channels == capture.channels => {
            Some(render.channels)
        }
        (Some(stream), None) | (None, Some(stream)) => Some(stream.channels),
        _ => None,
    }
}

/// Lists the intersection of channel capabilities for the existing directions.
pub(super) fn channel_choices(
    device: &VirtualEndpointDeviceSettings,
    settings: &VirtualEndpointSettings,
) -> Vec<u16> {
    let mut options = match (device.spec.render, device.spec.capture) {
        (Some(_), Some(_)) => settings
            .render_channels
            .iter()
            .copied()
            .filter(|count| settings.capture_channels.contains(count))
            .collect(),
        (Some(_), None) => settings.render_channels.clone(),
        (None, Some(_)) => settings.capture_channels.clone(),
        (None, None) => Vec::new(),
    };
    options.sort_unstable();
    options.dedup();
    options
}

/// Returns a PCM representation only when all existing directions have matching readback.
pub(super) fn current_sample_format(
    device: &VirtualEndpointDeviceSettings,
) -> Option<NativeSampleFormat> {
    let render = device
        .native_formats
        .render
        .current
        .map(|format| format.sample_format);
    let capture = device
        .native_formats
        .capture
        .current
        .map(|format| format.sample_format);
    match (device.spec.render, device.spec.capture) {
        (Some(_), Some(_)) if render == capture => render,
        (Some(_), None) => render,
        (None, Some(_)) => capture,
        _ => None,
    }
}

/// Lists PCM representations supported at the observed rate and each direction's channels.
pub(super) fn pcm_choices(device: &VirtualEndpointDeviceSettings) -> Vec<NativeSampleFormat> {
    let Some(rate) = device.observed_sample_rate else {
        return Vec::new();
    };
    if device.spec.render.is_none() && device.spec.capture.is_none() {
        return Vec::new();
    }
    [
        NativeSampleFormat::Signed16,
        NativeSampleFormat::Signed24,
        NativeSampleFormat::Signed32,
        NativeSampleFormat::Float32,
    ]
    .into_iter()
    .filter(|sample_format| {
        [
            (device.spec.render, &device.native_formats.render),
            (device.spec.capture, &device.native_formats.capture),
        ]
        .into_iter()
        .all(|(stream, formats)| {
            let Some(stream) = stream else {
                return true;
            };
            if formats.current.is_none() {
                return false;
            }
            formats.supported.iter().any(|format| {
                format.sample_rate == rate
                    && format.channels == stream.channels
                    && format.sample_format == *sample_format
            })
        })
    })
    .collect()
}

/// Names bit depth while distinguishing integer and float representations.
pub(super) const fn pcm_label(format: NativeSampleFormat) -> &'static str {
    match format {
        NativeSampleFormat::Signed16 => "16 bit",
        NativeSampleFormat::Signed24 => "24 bit",
        NativeSampleFormat::Signed32 => "32 bit (integer)",
        NativeSampleFormat::Float32 => "32 bit (float)",
    }
}

/// Formats Hz as kHz, without redundant decimals for integral kilohertz.
pub(super) fn rate_label(rate: u32) -> String {
    if rate.is_multiple_of(1_000) {
        format!("{} kHz", rate / 1_000)
    } else {
        format!("{} kHz", f64::from(rate) / 1_000.0)
    }
}

/// Derives one supported device change from fresh readback at the displayed revision.
/// Returns an error for changed revisions, unavailable devices, or unsupported values.
pub(super) fn command(
    snapshot: &DriverSnapshot,
    action: &SettingAction,
) -> Result<DriverCommand, String> {
    if snapshot.state != DriverState::Ready {
        return Err("Driver is unavailable".into());
    }
    if snapshot.revision != action.expected_revision {
        return Err("Device settings changed; choose again".into());
    }
    let device = snapshot
        .settings
        .devices
        .iter()
        .find(|device| device.spec.id == action.endpoint_id)
        .ok_or("The device is no longer available")?;
    if let SettingChoice::SampleFormat(sample_format) = action.choice {
        if !pcm_choices(device).contains(&sample_format) {
            return Err("The native PCM format is no longer supported".into());
        }
        return Ok(DriverCommand::SetSampleFormat {
            expected_revision: snapshot.revision,
            endpoint_id: action.endpoint_id.clone(),
            sample_format,
        });
    }
    configure(snapshot, device, action.choice)
}

fn configure(
    snapshot: &DriverSnapshot,
    device: &VirtualEndpointDeviceSettings,
    choice: SettingChoice,
) -> Result<DriverCommand, String> {
    let mut spec = device.spec.clone();
    match choice {
        SettingChoice::Channels(channels) => {
            spec.default_sample_rate = device
                .observed_sample_rate
                .ok_or("The current sample rate is unavailable")?;
            if !channel_choices(device, &snapshot.settings).contains(&channels) {
                return Err("The channel count is no longer supported".into());
            }
            for stream in [&mut spec.render, &mut spec.capture].into_iter().flatten() {
                stream.channels = channels;
                stream.channel_layout = match channels {
                    1 => ChannelLayout::Mono,
                    2 => ChannelLayout::Stereo,
                    _ => ChannelLayout::Discrete,
                };
            }
        }
        SettingChoice::SampleRate(rate) => {
            if !device.can_set_sample_rate || !device.supported_sample_rates.contains(&rate) {
                return Err("The sample rate is no longer supported".into());
            }
            spec.default_sample_rate = rate;
        }
        SettingChoice::SampleFormat(_) => {
            return Err("PCM changes require a native format command".into());
        }
    }
    Ok(DriverCommand::Configure {
        expected_revision: snapshot.revision,
        spec,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_audio_io::{
        NativeDeviceFormats, NativeStreamFormat, NativeStreamFormats, StreamSpec,
        VirtualEndpointSpec,
    };

    fn snapshot() -> DriverSnapshot {
        let current = NativeStreamFormat {
            sample_rate: 48_000,
            channels: 2,
            sample_format: NativeSampleFormat::Float32,
        };
        DriverSnapshot {
            protocol_version: 1,
            revision: 12,
            state: DriverState::Ready,
            installed_version: None,
            owner_process_id: None,
            persistence_error: None,
            settings: VirtualEndpointSettings {
                can_create: false,
                can_rename: true,
                render_channels: vec![1, 2, 8, 2],
                capture_channels: vec![2, 4],
                creation_template: None,
                devices: vec![VirtualEndpointDeviceSettings {
                    spec: VirtualEndpointSpec {
                        id: VirtualEndpointId::from("device"),
                        display_name: "IO".into(),
                        render: Some(StreamSpec {
                            channels: 2,
                            channel_layout: ChannelLayout::Stereo,
                        }),
                        capture: Some(StreamSpec {
                            channels: 4,
                            channel_layout: ChannelLayout::Discrete,
                        }),
                        supported_sample_rates: vec![44_100, 48_000, 96_000],
                        default_sample_rate: 44_100,
                    },
                    native_device_ids: vec!["native".into()],
                    observed_sample_rate: Some(48_000),
                    supported_sample_rates: vec![48_000, 96_000],
                    can_set_sample_rate: true,
                    native_formats: NativeDeviceFormats {
                        render: NativeStreamFormats {
                            current: Some(current),
                            supported: vec![
                                current,
                                NativeStreamFormat {
                                    sample_format: NativeSampleFormat::Signed32,
                                    ..current
                                },
                                NativeStreamFormat {
                                    sample_rate: 96_000,
                                    ..current
                                },
                                current,
                            ],
                        },
                        capture: NativeStreamFormats {
                            current: Some(NativeStreamFormat {
                                channels: 4,
                                ..current
                            }),
                            supported: vec![NativeStreamFormat {
                                channels: 4,
                                ..current
                            }],
                        },
                    },
                }],
            },
        }
    }

    fn action(choice: SettingChoice) -> SettingAction {
        SettingAction {
            endpoint_id: VirtualEndpointId::from("device"),
            expected_revision: 12,
            choice,
        }
    }

    #[test]
    fn choices_intersect_existing_directions_and_preserve_pcm_representation() {
        let snapshot = snapshot();
        let device = &snapshot.settings.devices[0];
        assert_eq!(channel_choices(device, &snapshot.settings), vec![2]);
        assert_eq!(pcm_choices(device), vec![NativeSampleFormat::Float32]);
        assert_eq!(current_channels(device), None);
        assert_eq!(
            current_sample_format(device),
            Some(NativeSampleFormat::Float32)
        );
        assert_eq!(pcm_label(NativeSampleFormat::Signed16), "16 bit");
        assert_eq!(pcm_label(NativeSampleFormat::Signed24), "24 bit");
        assert_eq!(pcm_label(NativeSampleFormat::Signed32), "32 bit (integer)");
        assert_eq!(pcm_label(NativeSampleFormat::Float32), "32 bit (float)");
    }

    #[test]
    fn channels_configure_all_existing_directions_at_observed_rate() {
        let snapshot = snapshot();
        let DriverCommand::Configure {
            expected_revision,
            spec,
        } = command(&snapshot, &action(SettingChoice::Channels(2))).unwrap()
        else {
            panic!("channels must configure the existing device");
        };
        assert_eq!(expected_revision, 12);
        assert_eq!(spec.default_sample_rate, 48_000);
        assert_eq!(spec.render, spec.capture);
        assert_eq!(spec.render.unwrap().channel_layout, ChannelLayout::Stereo);
        assert_eq!(spec.display_name, "IO");
    }

    #[test]
    fn single_direction_never_creates_the_missing_stream() {
        let mut snapshot = snapshot();
        snapshot.settings.devices[0].spec.capture = None;
        assert_eq!(
            channel_choices(&snapshot.settings.devices[0], &snapshot.settings),
            vec![1, 2, 8]
        );
        assert_eq!(
            pcm_choices(&snapshot.settings.devices[0]),
            vec![NativeSampleFormat::Signed32, NativeSampleFormat::Float32]
        );
        for (channels, layout) in [
            (1, ChannelLayout::Mono),
            (2, ChannelLayout::Stereo),
            (8, ChannelLayout::Discrete),
        ] {
            let DriverCommand::Configure { spec, .. } =
                command(&snapshot, &action(SettingChoice::Channels(channels))).unwrap()
            else {
                panic!("channels must configure the device");
            };
            assert_eq!(spec.capture, None);
            assert_eq!(spec.render.unwrap().channel_layout, layout);
        }
    }

    #[test]
    fn mixed_current_values_never_claim_a_checked_choice() {
        let mut snapshot = snapshot();
        let device = &mut snapshot.settings.devices[0];
        device
            .native_formats
            .capture
            .current
            .as_mut()
            .unwrap()
            .sample_format = NativeSampleFormat::Signed32;
        assert_eq!(current_channels(device), None);
        assert_eq!(current_sample_format(device), None);
    }

    #[test]
    fn capture_only_uses_capture_support_and_empty_devices_offer_no_choices() {
        let mut snapshot = snapshot();
        let device = &mut snapshot.settings.devices[0];
        device.spec.render = None;
        assert_eq!(current_channels(device), Some(4));
        assert_eq!(pcm_choices(device), vec![NativeSampleFormat::Float32]);
        assert_eq!(
            channel_choices(&snapshot.settings.devices[0], &snapshot.settings),
            vec![2, 4]
        );
        let DriverCommand::Configure { spec, .. } =
            command(&snapshot, &action(SettingChoice::Channels(4))).unwrap()
        else {
            panic!("channels must configure the capture-only device");
        };
        assert_eq!(spec.render, None);
        assert_eq!(spec.capture.unwrap().channels, 4);
        snapshot.settings.devices[0].spec.capture = None;
        assert!(pcm_choices(&snapshot.settings.devices[0]).is_empty());
        assert!(channel_choices(&snapshot.settings.devices[0], &snapshot.settings).is_empty());
    }

    #[test]
    fn pcm_support_at_other_rates_or_channels_is_not_offered() {
        let mut snapshot = snapshot();
        let device = &mut snapshot.settings.devices[0];
        let capture = device.native_formats.capture.supported[0];
        device.native_formats.capture.supported = vec![
            NativeStreamFormat {
                sample_rate: 96_000,
                ..capture
            },
            NativeStreamFormat {
                channels: 2,
                ..capture
            },
        ];
        assert!(pcm_choices(device).is_empty());
        assert!(
            command(
                &snapshot,
                &action(SettingChoice::SampleFormat(NativeSampleFormat::Float32))
            )
            .is_err()
        );
    }

    #[test]
    fn pcm_choices_require_native_readback_for_each_present_direction() {
        for render_missing in [true, false] {
            let mut snapshot = snapshot();
            let device = &mut snapshot.settings.devices[0];
            if render_missing {
                device.native_formats.render.current = None;
            } else {
                device.native_formats.capture.current = None;
            }
            assert!(pcm_choices(device).is_empty());
            assert!(
                command(
                    &snapshot,
                    &action(SettingChoice::SampleFormat(NativeSampleFormat::Float32))
                )
                .is_err()
            );
        }
        let mut snapshot = snapshot();
        let device = &mut snapshot.settings.devices[0];
        device.spec.capture = None;
        device.native_formats.capture.current = None;
        assert!(!pcm_choices(device).is_empty());
    }

    #[test]
    fn rate_change_preserves_geometry_and_uses_kilohertz_labels() {
        let snapshot = snapshot();
        let DriverCommand::Configure { spec, .. } =
            command(&snapshot, &action(SettingChoice::SampleRate(96_000))).unwrap()
        else {
            panic!("rate must configure the device");
        };
        assert_eq!(spec.default_sample_rate, 96_000);
        assert_eq!(spec.render, snapshot.settings.devices[0].spec.render);
        assert_eq!(spec.capture, snapshot.settings.devices[0].spec.capture);
        assert_eq!(rate_label(44_100), "44.1 kHz");
        assert_eq!(rate_label(48_000), "48 kHz");
        assert_eq!(rate_label(96_000), "96 kHz");
    }

    #[test]
    fn pcm_change_uses_one_revision_checked_command() {
        let snapshot = snapshot();
        assert_eq!(
            command(
                &snapshot,
                &action(SettingChoice::SampleFormat(NativeSampleFormat::Float32))
            )
            .unwrap(),
            DriverCommand::SetSampleFormat {
                expected_revision: 12,
                endpoint_id: VirtualEndpointId::from("device"),
                sample_format: NativeSampleFormat::Float32
            }
        );
        assert!(
            command(
                &snapshot,
                &action(SettingChoice::SampleFormat(NativeSampleFormat::Signed32))
            )
            .is_err()
        );
    }

    #[test]
    fn stale_missing_and_unsupported_settings_are_rejected() {
        let mut snapshot = snapshot();
        snapshot.revision = 13;
        assert_eq!(
            command(&snapshot, &action(SettingChoice::Channels(2))).unwrap_err(),
            "Device settings changed; choose again"
        );
        snapshot.revision = 12;
        assert!(command(&snapshot, &action(SettingChoice::Channels(8))).is_err());
        assert!(command(&snapshot, &action(SettingChoice::SampleRate(12_345))).is_err());
        snapshot.settings.devices.clear();
        assert!(command(&snapshot, &action(SettingChoice::Channels(2))).is_err());
    }

    #[test]
    fn unavailable_geometry_and_capabilities_reject_mutations() {
        let mut snapshot = snapshot();
        snapshot.settings.devices[0].observed_sample_rate = None;
        assert!(pcm_choices(&snapshot.settings.devices[0]).is_empty());
        assert!(command(&snapshot, &action(SettingChoice::Channels(2))).is_err());
        snapshot.settings.devices[0].can_set_sample_rate = false;
        assert!(command(&snapshot, &action(SettingChoice::SampleRate(96_000))).is_err());
        snapshot.state = DriverState::InstalledNotLoaded;
        assert_eq!(
            command(&snapshot, &action(SettingChoice::Channels(2))).unwrap_err(),
            "Driver is unavailable"
        );
    }
}

use mixrelay_audio_graph::NodeConfig;
use mixrelay_audio_io::{
    ApplicationCaptureRequest, ApplicationSelector, ChannelLayout, HostAudioBackendError,
    StreamSpec,
};
use mixrelay_engine_service::graph_composition::SharedSource;

/// The native process tap mixes an exact bundle into stereo; it does not walk
/// process descendants. Local channel projection and gain belong to graph DSP.
pub(super) fn resolve(
    request: &ApplicationCaptureRequest,
) -> Result<SharedSource, HostAudioBackendError> {
    request
        .validate_contract(2)
        .map_err(|error| unsupported(&error.to_string()))?;
    let ApplicationSelector::BundleId {
        bundle_id,
        include_descendants: false,
    } = &request.selector
    else {
        return Err(unsupported(
            "macOS capture requires exact bundle selection without descendants",
        ));
    };
    if request.requires_reselection {
        return Err(unsupported(
            "application capture requires explicit reselection",
        ));
    }
    let mut canonical = request.clone();
    canonical.gain_db = 0.0;
    canonical.stream = StreamSpec {
        channels: 2,
        channel_layout: ChannelLayout::Stereo,
    };
    // Playback policy remains configuration, not a second identity. Conflicting
    // policies for one bundle must fail catalog composition instead of opening
    // two taps that could mute each other's shared application.
    Ok(SharedSource {
        id: bundle_id.clone(),
        config: NodeConfig::ApplicationCapture(canonical),
    })
}

fn unsupported(capability: &str) -> HostAudioBackendError {
    HostAudioBackendError::CapabilityUnavailable {
        capability: capability.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_audio_io::SourcePlaybackPolicy;

    fn request() -> ApplicationCaptureRequest {
        ApplicationCaptureRequest {
            selector: ApplicationSelector::BundleId {
                bundle_id: "org.mixrelay.capture-fixture".into(),
                include_descendants: false,
            },
            stream: StreamSpec {
                channels: 2,
                channel_layout: ChannelLayout::Stereo,
            },
            source_playback: SourcePlaybackPolicy::Preserve,
            gain_db: 0.0,
            requires_reselection: false,
        }
    }

    #[test]
    fn local_mono_projection_and_gain_share_one_stereo_native_owner() {
        let stereo = request();
        let mut mono = stereo.clone();
        mono.stream.channels = 1;
        mono.stream.channel_layout = ChannelLayout::Mono;
        mono.gain_db = -12.0;
        assert_eq!(resolve(&stereo).unwrap(), resolve(&mono).unwrap());
        assert_eq!(mono.stream.channels, 1);
        assert_eq!(mono.gain_db.to_bits(), (-12.0_f32).to_bits());
        let NodeConfig::ApplicationCapture(canonical) = resolve(&mono).unwrap().config else {
            panic!("application source")
        };
        assert_eq!(canonical.stream.channels, 2);
        assert_eq!(canonical.gain_db.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn conflicting_playback_is_one_identity_with_different_configuration() {
        let preserve = resolve(&request()).unwrap();
        let mut muted = request();
        muted.source_playback = SourcePlaybackPolicy::MuteWhileCaptured;
        let muted = resolve(&muted).unwrap();
        assert_eq!(preserve.id, muted.id);
        assert_ne!(preserve.config, muted.config);
        let mut other = request();
        other.selector = ApplicationSelector::BundleId {
            bundle_id: "org.mixrelay.other-fixture".into(),
            include_descendants: false,
        };
        assert_ne!(preserve.id, resolve(&other).unwrap().id);
    }

    #[test]
    fn unsupported_selectors_and_unresolved_intent_are_never_relabelled_as_bundle_capture() {
        for selector in [
            ApplicationSelector::BundleId {
                bundle_id: "org.mixrelay.capture-fixture".into(),
                include_descendants: true,
            },
            ApplicationSelector::ExecutablePath {
                path: "/Applications/Fixture.app".into(),
            },
            ApplicationSelector::ProcessId {
                process_id: 42,
                include_descendants: false,
            },
            ApplicationSelector::AllExcept { bundle_ids: vec![] },
        ] {
            let mut unsupported = request();
            unsupported.selector = selector;
            assert!(resolve(&unsupported).is_err());
        }
        let mut unresolved = request();
        unresolved.requires_reselection = true;
        assert!(resolve(&unresolved).is_err());
        let mut invalid = request();
        invalid.stream.channels = 3;
        assert!(resolve(&invalid).is_err());
        invalid = request();
        invalid.gain_db = f32::NAN;
        assert!(resolve(&invalid).is_err());
    }
}

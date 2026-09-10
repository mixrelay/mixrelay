use super::*;
use mixrelay_audio_io::{
    VirtualEndpointSettings,
    session::{AudioBackendKind, AudioSessionCapabilities, ClockAuthority, SessionFormat},
};

struct ConfigurableBackend(VirtualEndpointCapabilities);

impl Default for ConfigurableBackend {
    fn default() -> Self {
        Self(VirtualEndpointCapabilities {
            max_render_channels: 64,
            max_capture_channels: 64,
            supported_sample_rates: mixrelay_audio_io::formats::SAMPLE_RATES.to_vec(),
            supports_duplex: true,
            supports_dynamic_creation: true,
        })
    }
}

impl VirtualEndpointBackend for ConfigurableBackend {
    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.0
    }
    fn settings(&self) -> Option<VirtualEndpointSettings> {
        Some(VirtualEndpointSettings::configurable(self.capabilities()))
    }
    fn prepare_graph_attachments(
        &self,
        specs: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        if specs.iter().any(|spec| spec.display_name == "rejected") {
            return Err(HostAudioBackendError::BackendFailure {
                message: "native staging rejected".into(),
            });
        }
        Ok(())
    }
}

pub(super) fn controller(document: GraphDocument) -> EngineGraphController {
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 128,
    };
    EngineGraphController::new_with_initial_document(
        Arc::new(ProcessorExchange::new(document.revision)),
        EngineGraphControllerConfig {
            format,
            meters: default_peak_meter_bank(),
            initial_bindings: vec![],
            endpoint_backend: Arc::new(ConfigurableBackend::default()),
            required_endpoint_node_id: None,
            processor_catalog: ProcessorCatalog::default(),
            graph_resources: Arc::new(NoopGraphResourceBackend),
            runtime_status: Arc::new(session::AdapterRuntimeStatus::new(
                session::AdapterProfile {
                    initial_format: SessionFormat {
                        sample_rate: 48_000,
                        block_frames: 128,
                    },
                    capabilities: AudioSessionCapabilities {
                        backend: AudioBackendKind::ContractOnly,
                        supported_sample_rates: vec![44_100, 48_000],
                        supported_block_frames: vec![128],
                        can_reconfigure_live: false,
                        hardware_input: false,
                        hardware_output: false,
                        virtual_endpoint: true,
                        application_capture: false,
                        sample_rate_conversion: false,
                        drift_correction: false,
                    },
                    application_capture_rates: vec![],
                    clock_authority: ClockAuthority::EngineTimer,
                    transport_latency_frames: None,
                    requires_observed_device_rate: false,
                },
            )),
        },
        document,
    )
}

pub(super) fn settings(controller: &EngineGraphController) -> VirtualEndpointSettings {
    controller
        .status()
        .audio_session
        .unwrap()
        .virtual_endpoint_settings
        .unwrap()
}

pub(super) fn endpoint_document(revision: u64, name: &str) -> GraphDocument {
    GraphDocument::new(
        revision,
        vec![NodeSpec {
            control: mixrelay_audio_graph::NodeControl::default(),
            id: NodeId::from("graph-device"),
            label: Some(name.into()),
            config: NodeConfig::VirtualEndpoint(
                VirtualEndpointSpec {
                    id: VirtualEndpointId::from("7a96b0d1-532a-4893-a3ae-8c805f45de92"),
                    display_name: name.into(),
                    render: Some(StreamSpec {
                        channels: 64,
                        channel_layout: ChannelLayout::Discrete,
                    }),
                    capture: Some(StreamSpec {
                        channels: 16,
                        channel_layout: ChannelLayout::Discrete,
                    }),
                    supported_sample_rates: vec![44_100, 48_000, 96_000],
                    default_sample_rate: 48_000,
                }
                .into(),
            ),
        }],
        vec![],
    )
}

#[test]
fn rejected_staging_preserves_devices_and_final_removal_keeps_only_creation_choices() {
    let initial = endpoint_document(1, "SPAT send-return");
    let controller = controller(initial.clone());
    let expected = settings(&controller);
    assert_eq!(expected.devices.len(), 1);
    assert_eq!(expected.devices[0].spec.display_name, "SPAT send-return");
    assert!(expected.creation_template.is_some());
    let mut rejected = initial;
    rejected.revision = 2;
    let NodeConfig::VirtualEndpoint(spec) = &mut rejected.nodes[0].config else {
        panic!()
    };
    spec.display_name = "rejected".into();
    assert!(controller.apply_graph(rejected).is_err());
    assert_eq!(settings(&controller), expected);
    controller
        .apply_graph(GraphDocument::new(2, vec![], vec![]))
        .unwrap();
    assert!(controller.graph().nodes.is_empty());
    assert!(settings(&controller).devices.is_empty());
    assert_eq!(
        settings(&controller).creation_template,
        expected.creation_template
    );
    assert!(settings(&controller).can_create);
}

#[test]
fn partial_reference_removal_never_changes_shared_device_stream_configuration() {
    let mut initial = endpoint_document(1, "Independent streams");
    let controller = controller(initial.clone());
    let expected = settings(&controller).devices;
    initial.revision = 2;
    let NodeConfig::VirtualEndpoint(config) = &mut initial.nodes[0].config else {
        panic!()
    };
    config.reference.capture = false;
    controller.apply_graph(initial).unwrap();
    assert_eq!(settings(&controller).devices, expected);
    let graph = controller.graph();
    let NodeConfig::VirtualEndpoint(config) = &graph.nodes[0].config else {
        panic!("endpoint fixture")
    };
    assert!(config.reference.render);
    assert!(!config.reference.capture);
    assert_eq!(config.spec.capture.unwrap().channels, 16);
    assert_eq!(
        settings(&controller).render_channels,
        (1..=64).collect::<Vec<_>>()
    );
}

#[test]
fn empty_startup_offers_new_identity_creation_without_inventing_a_device() {
    let empty = controller(GraphDocument::new(1, vec![], vec![]));
    assert!(settings(&empty).can_create);
    assert!(settings(&empty).creation_template.is_some());
    assert!(settings(&empty).devices.is_empty());
    assert!(empty.graph().nodes.is_empty());
}

#[test]
fn settings_wire_roundtrip_exposes_creation_choices_and_identified_devices() {
    let controller = controller(endpoint_document(1, "SPAT send-return"));
    let status = controller.status();
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(
        json["audioSession"]["virtualEndpointSettings"]["canRename"],
        true
    );
    assert_eq!(
        serde_json::from_value::<AppStatus>(json.clone()).unwrap(),
        status
    );
    let settings = &json["audioSession"]["virtualEndpointSettings"];
    assert!(settings["creationTemplate"].is_object());
    assert_eq!(settings["devices"].as_array().unwrap().len(), 1);
    assert_eq!(
        settings["devices"][0]["spec"]["displayName"],
        "SPAT send-return"
    );
    assert!(settings.get("defaultEndpoint").is_none());
}

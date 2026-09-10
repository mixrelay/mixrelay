//! Real controller/disk/DSP regression with a recording device-contract adapter.
//! This does not open the host's installed virtual device or claim native re-open evidence.
use super::*;
use crate::endpoint_settings_tests::controller;
use std::fs;

#[derive(Default)]
struct RecordingEndpoint {
    contract: ContractOnlyVirtualEndpointBackend,
    current: parking_lot::Mutex<Vec<VirtualEndpointSpec>>,
    prepares: std::sync::atomic::AtomicUsize,
    changed_specs: std::sync::atomic::AtomicUsize,
}

impl VirtualEndpointBackend for RecordingEndpoint {
    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        self.contract.capabilities()
    }

    fn prepare_graph_attachments(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        use std::sync::atomic::Ordering;
        self.prepares.fetch_add(1, Ordering::Relaxed);
        let mut current = self.current.lock();
        if *current != desired {
            *current = desired.to_vec();
            self.changed_specs.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[test]
fn virtual_gain_survives_disk_restart_and_does_not_change_prepared_device_specs() {
    use std::sync::atomic::Ordering;
    let fixture = tonenari_test_support::FixtureDir::new("source-gain").unwrap();
    let path = fixture.join("graphs.json");
    let initial = default_duplex_document(1, None);
    let NodeConfig::VirtualEndpoint(config) = &initial.nodes[0].config else {
        unreachable!()
    };
    let backend = Arc::new(RecordingEndpoint::default());
    *backend.current.lock() = vec![config.spec.clone()];
    let mut engine = controller(initial).with_graph_catalog_path(path.clone());
    engine.endpoint_backend = backend.clone();
    for gain_db in [-6.0, -12.0] {
        let mut document = engine.graph();
        document.revision += 1;
        let NodeConfig::VirtualEndpoint(endpoint) = &mut document.nodes[0].config else {
            unreachable!()
        };
        endpoint.gain_db = gain_db;
        assert!(
            engine
                .apply_graph(document)
                .unwrap()
                .persistence_error
                .is_none()
        );
        let mut processor = crate::controller_test_support::settle(&engine);
        let source = [1.0, -1.0, 0.5, -0.5];
        let mut sink = [0.0; 4];
        processor
            .process_block(2, &[&source], &mut [&mut sink])
            .unwrap();
        assert_eq!(sink.map(f32::to_bits), source.map(f32::to_bits));
        // The fixture explicitly routes this device back into itself. App audio
        // enters the bus unchanged; only the graph-local return is attenuated.
        processor
            .process_block(2, &[&source], &mut [&mut sink])
            .unwrap();
        for (input, output) in source.into_iter().zip(sink) {
            let expected = input + input * 10.0_f32.powf(gain_db / 20.0);
            assert!((output - expected).abs() < 0.000_001);
        }
    }
    assert_eq!(backend.prepares.load(Ordering::Relaxed), 2);
    assert_eq!(backend.changed_specs.load(Ordering::Relaxed), 0);
    assert!(fs::read_to_string(&path).unwrap().contains("gainDb"));
    drop(engine);
    let restarted = controller(GraphDocument::new(1, vec![], vec![])).with_graph_catalog_path(path);
    let restored = restarted.graph();
    let NodeConfig::VirtualEndpoint(endpoint) = &restored.nodes[0].config else {
        unreachable!()
    };
    assert_eq!(endpoint.gain_db.to_bits(), (-12.0_f32).to_bits());
    assert_eq!(endpoint.spec, backend.current.lock()[0]);
}

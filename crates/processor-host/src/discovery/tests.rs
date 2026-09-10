use super::*;

fn locator(format: RackFormat, id: &str) -> RackLocator {
    serde_json::from_value(serde_json::json!({
        "format":format.argument(), "uniqueId":id, "path":format!("/fixture/{id}"),
        "name":id, "vendor":"fixture", "version":1, "category":"effect",
        "hasEditor":false, "acceptsMidi":false
    }))
    .unwrap()
}

fn record(locator: &RackLocator) -> rack_formats::RackProbeRecord {
    rack_formats::RackProbeRecord {
        descriptor: mixrelay_control_protocol::ExternalProcessorDescriptor {
            type_id: format!("{}:{}", locator.format.argument(), locator.unique_id),
            display_name: locator.unique_id.clone(),
            vendor: "fixture".to_owned(),
            version: "1".to_owned(),
            format: locator.format.protocol(),
            input_channels: 2,
            output_channels: 2,
            has_editor: false,
            parameters: Vec::new(),
        },
        locator: locator.clone(),
        layout_index: 0,
    }
}

#[test]
fn successful_empty_is_distinct_from_failed_format_and_retry_clears_diagnostics() {
    let catalog = ProcessorCatalog::default();
    let candidate = locator(RackFormat::Clap, "effect");
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(vec![candidate.clone()]),
        |candidate| Ok(record(candidate)),
    );
    let initial = catalog.protocol_catalog();
    assert_eq!(initial.processors.len(), 1);
    for message in [
        "child timed out",
        "invalid child JSON",
        "child exited with 7",
    ] {
        rescan_with(
            &catalog,
            &[RackFormat::Clap],
            |_| Err(message.to_owned()),
            |_| unreachable!(),
        );
        let failed = catalog.protocol_catalog();
        assert_eq!(failed.processors, initial.processors);
        assert_eq!(failed.diagnostics.len(), 1);
        assert_eq!(failed.diagnostics[0].stage, ProcessorDiscoveryStage::Scan);
        assert_eq!(failed.diagnostics[0].message, message);
        assert!(failed.diagnostics[0].plugin_id.is_none());
    }
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(Vec::new()),
        |_| unreachable!(),
    );
    let retried = catalog.protocol_catalog();
    assert!(retried.processors.is_empty());
    assert!(retried.diagnostics.is_empty());
}

#[test]
fn partial_probe_failure_retains_only_exact_candidate_and_successful_formats_refresh() {
    let catalog = ProcessorCatalog::default();
    let failed = locator(RackFormat::Clap, "failed");
    let removed = locator(RackFormat::Clap, "removed");
    let new = locator(RackFormat::Vst3, "new");
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(vec![failed.clone(), removed.clone()]),
        |candidate| Ok(record(candidate)),
    );
    let mut probes = 0;
    rescan_with(
        &catalog,
        &[RackFormat::Clap, RackFormat::Vst3],
        |format| {
            Ok(if format == RackFormat::Clap {
                vec![failed.clone(), failed.clone()]
            } else {
                vec![new.clone()]
            })
        },
        |candidate| {
            probes += 1;
            if candidate.unique_id == "failed" {
                Err("load failed".to_owned())
            } else {
                Ok(record(candidate))
            }
        },
    );
    let partial = catalog.protocol_catalog();
    assert_eq!(
        probes, 2,
        "overlapping default/custom paths probe an identity only once"
    );
    assert_eq!(
        partial
            .processors
            .iter()
            .map(|entry| entry.type_id.as_str())
            .collect::<Vec<_>>(),
        vec!["clap:failed", "vst3:new"]
    );
    assert_eq!(partial.diagnostics.len(), 1);
    assert_eq!(
        partial.diagnostics[0].plugin_id.as_deref(),
        Some("clap:failed")
    );
    assert_eq!(partial.diagnostics[0].stage, ProcessorDiscoveryStage::Probe);

    let mut replaced = failed.clone();
    replaced.path = PathBuf::from("/different/path");
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(vec![replaced.clone()]),
        |_| Err("load failed".to_owned()),
    );
    assert!(
        catalog.protocol_catalog().processors.is_empty(),
        "same ID from a different bundle must not retain a stale locator"
    );
}

#[test]
fn successful_probe_reclassification_removes_old_effect_without_a_failure() {
    let catalog = ProcessorCatalog::default();
    let candidate = locator(RackFormat::Clap, "effect");
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(vec![candidate.clone()]),
        |candidate| Ok(record(candidate)),
    );
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(vec![candidate.clone()]),
        |candidate| {
            let mut record = record(candidate);
            record.descriptor.input_channels = 0;
            Ok(record)
        },
    );
    assert!(catalog.protocol_catalog().processors.is_empty());
    assert!(catalog.protocol_catalog().diagnostics.is_empty());
}

#[test]
fn catalog_readers_keep_atomic_previous_snapshot_during_scan() {
    let catalog = ProcessorCatalog::default();
    let candidate = locator(RackFormat::Clap, "effect");
    rescan_with(
        &catalog,
        &[RackFormat::Clap],
        |_| Ok(vec![candidate.clone()]),
        |candidate| Ok(record(candidate)),
    );
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let owner = catalog.clone();
    let scan = std::thread::spawn(move || {
        rescan_with(
            &owner,
            &[RackFormat::Clap],
            |_| {
                entered_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Err("scan failed".to_owned())
            },
            |_| unreachable!(),
        );
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let during = catalog.protocol_catalog();
    assert_eq!(during.processors.len(), 1);
    assert!(during.diagnostics.is_empty());
    resume_tx.send(()).unwrap();
    scan.join().unwrap();
    let after = catalog.protocol_catalog();
    assert_eq!(after.processors, during.processors);
    assert_eq!(after.diagnostics.len(), 1);
}

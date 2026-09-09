//! Physical/source/sink I/O ownership, prepared off the real-time thread.
//! Graph gains are DSP parameters, never part of native stream identity.
use super::native::NativeStream;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tonenari_audio_graph::{
    AudioDeviceInputConfig, AudioDeviceOutputConfig, GraphDocument, NodeConfig,
};
use tonenari_audio_io::{
    DeviceDirection,
    process::{ProcessReport, TransferOutcome},
};

#[derive(Clone, Default, PartialEq)]
pub(super) struct DeviceSpec {
    inputs: Vec<(String, AudioDeviceInputConfig)>,
    outputs: Vec<(String, AudioDeviceOutputConfig)>,
}

impl DeviceSpec {
    pub(super) fn from_document(document: &GraphDocument) -> Self {
        let mut spec = Self::default();
        for node in &document.nodes {
            match &node.config {
                NodeConfig::AudioDeviceInput(config) => {
                    let mut config = config.clone();
                    config.gain_db = 0.0;
                    spec.inputs.push((node.id.as_str().into(), config));
                }
                NodeConfig::AudioDeviceOutput(config) => {
                    spec.outputs.push((node.id.as_str().into(), config.clone()));
                }
                _ => {}
            }
        }
        spec
    }
}

#[derive(Default)]
pub(super) struct DeviceStreams {
    pub(super) generation: u64,
    inputs: BTreeMap<String, ObservedStream>,
    outputs: BTreeMap<String, ObservedStream>,
    format: Option<tonenari_audio_graph::RenderFormat>,
}

struct ObservedStream {
    stream: NativeStream,
    connected: Arc<AtomicBool>,
}
impl DeviceStreams {
    pub(super) fn discard_inputs(&mut self) {
        for entry in self.inputs.values_mut() {
            entry.stream.discard_input();
        }
    }
    pub(super) fn shares_clock(&self, id: u32, rate: u32) -> bool {
        self.inputs
            .values()
            .chain(self.outputs.values())
            .all(|entry| entry.stream.shares_clock(id, rate))
    }
}
impl ObservedStream {
    fn new(stream: NativeStream) -> Self {
        Self {
            stream,
            connected: Arc::new(AtomicBool::new(true)),
        }
    }
}

/// Read-only health shared with the recovery worker, never a shared stream/ring.
#[derive(Clone)]
pub(super) struct DeviceWatch {
    pub(super) generation: u64,
    spec: DeviceSpec,
    inputs: BTreeMap<String, Arc<AtomicBool>>,
    outputs: BTreeMap<String, Arc<AtomicBool>>,
    pub(super) format: tonenari_audio_graph::RenderFormat,
}
impl DeviceWatch {
    pub(super) fn disconnected(&self) -> DeviceSpec {
        DeviceSpec {
            inputs: self
                .spec
                .inputs
                .iter()
                .filter(|(id, _)| {
                    self.inputs
                        .get(id)
                        .is_some_and(|value| !value.load(Ordering::Acquire))
                })
                .cloned()
                .collect(),
            outputs: self
                .spec
                .outputs
                .iter()
                .filter(|(id, _)| {
                    self.outputs
                        .get(id)
                        .is_some_and(|value| !value.load(Ordering::Acquire))
                })
                .cloned()
                .collect(),
        }
    }
}
impl DeviceSpec {
    pub(super) fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty()
    }
}

impl DeviceStreams {
    #[cfg(test)]
    pub(super) fn open(spec: &DeviceSpec, generation: u64) -> Result<Self, String> {
        Self::open_format(spec, generation, super::render_format())
    }
    pub(super) fn open_format(
        spec: &DeviceSpec,
        generation: u64,
        format: tonenari_audio_graph::RenderFormat,
    ) -> Result<Self, String> {
        if spec.inputs.is_empty() && spec.outputs.is_empty() {
            return Ok(Self {
                generation,
                format: Some(format),
                ..Self::default()
            });
        }
        let targets = tonenari_audio_io::linux::targets().map_err(|error| error.to_string())?;
        Self::open_inventory(spec, generation, &targets, false, None, format)
    }

    pub(super) fn recover(
        spec: &DeviceSpec,
        generation: u64,
        stopping: &AtomicBool,
        format: tonenari_audio_graph::RenderFormat,
    ) -> Result<Self, String> {
        let targets = tonenari_audio_io::linux::targets().map_err(|error| error.to_string())?;
        // Devices can return independently. One absent interface must not delay
        // another interface's recovery. Unresolvable requests remain watched.
        let available = DeviceSpec {
            inputs: spec
                .inputs
                .iter()
                .take_while(|_| !stopping.load(Ordering::Acquire))
                .filter(|(_, config)| {
                    tonenari_audio_io::linux::resolve_device_target(
                        &targets,
                        &config.selector,
                        DeviceDirection::Input,
                        config.output_channels,
                    )
                    .is_ok()
                })
                .cloned()
                .collect(),
            outputs: spec
                .outputs
                .iter()
                .take_while(|_| !stopping.load(Ordering::Acquire))
                .filter(|(_, config)| {
                    tonenari_audio_io::linux::resolve_device_target(
                        &targets,
                        &config.selector,
                        DeviceDirection::Output,
                        config.input_channels,
                    )
                    .is_ok()
                })
                .cloned()
                .collect(),
        };
        Self::open_inventory(
            &available,
            generation,
            &targets,
            true,
            Some(stopping),
            format,
        )
    }

    fn open_inventory(
        spec: &DeviceSpec,
        generation: u64,
        targets: &[tonenari_audio_io::linux::PipeWireTarget],
        partial: bool,
        stopping: Option<&AtomicBool>,
        format: tonenari_audio_graph::RenderFormat,
    ) -> Result<Self, String> {
        let check_cancelled = || {
            if stopping.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                Err("device recovery cancelled".to_owned())
            } else {
                Ok(())
            }
        };
        check_cancelled()?;
        // Validate the entire request before opening any native stream. The
        // registry serial pins this apply to the selected device, not a recycled ID.
        let inputs = spec
            .inputs
            .iter()
            .map(|(id, config)| {
                check_cancelled()?;
                tonenari_audio_io::linux::resolve_device_target(
                    targets,
                    &config.selector,
                    DeviceDirection::Input,
                    config.output_channels,
                )
                .map(|target| (id, config.output_channels, target))
                .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let outputs = spec
            .outputs
            .iter()
            .map(|(id, config)| {
                check_cancelled()?;
                tonenari_audio_io::linux::resolve_device_target(
                    targets,
                    &config.selector,
                    DeviceDirection::Output,
                    config.input_channels,
                )
                .map(|target| (id, config.input_channels, target))
                .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut streams = Self {
            generation,
            format: Some(format),
            ..Self::default()
        };
        for (id, channels, target) in inputs {
            check_cancelled()?;
            let opened = NativeStream::open_format(
                &format!("{}.device-input", super::sanitize_node_name(id)),
                &target.display_name,
                Some(&target.serial),
                3,
                channels,
                format,
            );
            match opened {
                Ok(stream) => {
                    streams
                        .inputs
                        .insert(id.clone(), ObservedStream::new(stream));
                }
                Err(error) if !partial => return Err(error),
                Err(_) => {}
            }
        }
        for (id, channels, target) in outputs {
            check_cancelled()?;
            let opened = NativeStream::open_format(
                &format!("{}.device-output", super::sanitize_node_name(id)),
                &target.display_name,
                Some(&target.serial),
                4,
                channels,
                format,
            );
            match opened {
                Ok(stream) => {
                    streams
                        .outputs
                        .insert(id.clone(), ObservedStream::new(stream));
                }
                Err(error) if !partial => return Err(error),
                Err(_) => {}
            }
        }
        // PAUSED only means registered, not linked. Bound the control-plane
        // handshake and drop every partial owner on failure; the DSP never waits.
        let started = Instant::now();
        while !streams.all_connected() {
            check_cancelled()?;
            if started.elapsed() >= Duration::from_secs(2) {
                if partial {
                    streams.inputs.retain(|_, value| value.stream.connected());
                    streams.outputs.retain(|_, value| value.stream.connected());
                    break;
                }
                return Err("PipeWire could not connect the selected audio device".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(streams)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty()
    }

    fn all_connected(&self) -> bool {
        self.inputs
            .values()
            .chain(self.outputs.values())
            .all(|value| value.stream.connected() && value.stream.ready())
    }

    pub(super) fn watch(&self, spec: DeviceSpec) -> DeviceWatch {
        DeviceWatch {
            format: self.format.unwrap_or_else(super::render_format),
            generation: self.generation,
            spec,
            inputs: self
                .inputs
                .iter()
                .map(|(id, value)| (id.clone(), value.connected.clone()))
                .collect(),
            outputs: self
                .outputs
                .iter()
                .map(|(id, value)| (id.clone(), value.connected.clone()))
                .collect(),
        }
    }

    /// RT-only swaps existing slots. Keep original health cells so the control
    /// watcher follows the replacement, and never reopen a still-healthy stream.
    pub(super) fn replace_disconnected(&mut self, next: &mut Self) {
        for (current, replacements) in [
            (&mut self.inputs, &mut next.inputs),
            (&mut self.outputs, &mut next.outputs),
        ] {
            for (id, replacement) in replacements {
                if let Some(value) = current.get_mut(id)
                    && !value.stream.connected()
                {
                    // Input prepared during a retry must not replay queued old
                    // microphone audio. Native output modes deliberately ignore this.
                    replacement.stream.discard_input();
                    std::mem::swap(&mut value.stream, &mut replacement.stream);
                    value
                        .connected
                        .store(value.stream.connected(), Ordering::Release);
                }
            }
        }
    }

    pub(super) fn read(&mut self, node: &str, samples: &mut [f32]) -> TransferOutcome {
        samples.fill(0.0);
        self.inputs
            .get_mut(node)
            .map_or(TransferOutcome::complete(0), |stream| {
                if !stream.stream.connected() {
                    return TransferOutcome {
                        frames: 0,
                        discontinuity: true,
                        ..TransferOutcome::complete(0)
                    };
                }
                TransferOutcome::complete(stream.stream.read(samples))
            })
    }

    pub(super) fn write(&mut self, node: &str, samples: &[f32]) -> TransferOutcome {
        self.outputs
            .get_mut(node)
            .map_or(TransferOutcome::complete(0), |stream| {
                if !stream.stream.connected() {
                    return TransferOutcome {
                        frames: 0,
                        discontinuity: true,
                        ..TransferOutcome::complete(0)
                    };
                }
                TransferOutcome::complete(stream.stream.write(samples))
            })
    }

    pub(super) fn take_report(&mut self) -> ProcessReport {
        let mut report = ProcessReport::default();
        for stream in self.inputs.values_mut().chain(self.outputs.values_mut()) {
            stream
                .connected
                .store(stream.stream.connected(), Ordering::Release);
            let next = stream.stream.take_report();
            report.input_starvations += next.input_starvations;
            report.output_overflows += next.output_overflows;
            report.discontinuities += next.discontinuities;
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_recovery_opens_no_native_resources() {
        let stopping = AtomicBool::new(true);
        assert!(
            DeviceStreams::open_inventory(
                &DeviceSpec::default(),
                3,
                &[],
                true,
                Some(&stopping),
                super::super::render_format()
            )
            .is_err()
        );
    }
    #[test]
    fn no_device_graph_does_not_need_a_pipewire_server() {
        let mut streams = DeviceStreams::open(&DeviceSpec::default(), 1).unwrap();
        let mut input = [1.0; 4];
        assert_eq!(streams.read("missing", &mut input).frames, 0);
        assert_eq!(input.map(f32::to_bits), [0; 4]);
        assert_eq!(streams.write("missing", &input).frames, 0);
    }
    #[test]
    fn gain_edits_do_not_reopen_native_devices() {
        let mut doc: GraphDocument = serde_json::from_value(serde_json::json!({
            "schemaVersion":1,"id":"test","revision":1,"nodes":[
                {"id":"input","type":"audio_device_input","config":{"selector":{"match":"default_input"},"outputChannels":64,"gainDb":0.0}}
            ],"edges":[]
        })).unwrap();
        let before = DeviceSpec::from_document(&doc);
        if let NodeConfig::AudioDeviceInput(config) = &mut doc.nodes[0].config {
            config.gain_db = -12.0;
        }
        assert!(before == DeviceSpec::from_document(&doc));
        if let NodeConfig::AudioDeviceInput(config) = &mut doc.nodes[0].config {
            config.output_channels = 16;
        }
        assert!(before != DeviceSpec::from_document(&doc));
    }
    #[test]
    fn health_watch_retries_only_disconnected_registered_streams() {
        let input = Arc::new(AtomicBool::new(true));
        let output = Arc::new(AtomicBool::new(true));
        let spec = DeviceSpec {
            inputs: vec![(
                "mic".into(),
                AudioDeviceInputConfig {
                    selector: tonenari_audio_io::DeviceSelector::DefaultInput,
                    output_channels: 64,
                    gain_db: 0.0,
                },
            )],
            outputs: vec![(
                "speaker".into(),
                AudioDeviceOutputConfig {
                    selector: tonenari_audio_io::DeviceSelector::DefaultOutput,
                    input_channels: 16,
                },
            )],
        };
        let watch = DeviceWatch {
            format: super::super::render_format(),
            generation: 3,
            spec,
            inputs: BTreeMap::from([("mic".into(), input.clone())]),
            outputs: BTreeMap::from([("speaker".into(), output.clone())]),
        };
        assert!(watch.disconnected().is_empty());
        input.store(false, Ordering::Release);
        assert_eq!(watch.disconnected().inputs.len(), 1);
        assert!(watch.disconnected().outputs.is_empty());
        input.store(true, Ordering::Release);
        output.store(false, Ordering::Release);
        assert!(watch.disconnected().inputs.is_empty());
        assert_eq!(watch.disconnected().outputs.len(), 1);
    }
}

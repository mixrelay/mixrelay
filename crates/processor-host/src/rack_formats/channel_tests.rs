use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use truce_rack_core::{
    PluginCore,
    error::{Error, Result as PluginResult},
    info::{ParameterInfo, PresetInfo},
};

use super::*;

struct ChannelFixture {
    info: PluginInfo,
    layouts: Vec<BusLayout>,
    active: Option<BusLayout>,
    activations: Arc<AtomicUsize>,
}

impl PluginCore for ChannelFixture {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn active_layout(&self) -> Option<&BusLayout> {
        self.active.as_ref()
    }

    fn supported_layouts(&self) -> &[BusLayout] {
        &self.layouts
    }

    fn parameter_count(&self) -> usize {
        0
    }

    fn parameter_info(&self, index: usize) -> PluginResult<ParameterInfo> {
        Err(Error::InvalidParameter(index))
    }

    fn parameter_value(&self, index: usize) -> PluginResult<f64> {
        Err(Error::InvalidParameter(index))
    }

    fn parameter_value_string(&self, index: usize, _value: f64) -> PluginResult<String> {
        Err(Error::InvalidParameter(index))
    }

    fn set_parameter(&mut self, index: usize, _value: f64) -> PluginResult<()> {
        Err(Error::InvalidParameter(index))
    }

    fn preset_count(&self) -> usize {
        0
    }

    fn preset_info(&self, index: usize) -> PluginResult<PresetInfo> {
        Err(Error::InvalidParameter(index))
    }

    fn load_preset(&mut self, _preset_number: i32) -> PluginResult<()> {
        Ok(())
    }

    fn save_state(&self) -> PluginResult<Vec<u8>> {
        Ok(Vec::new())
    }

    fn load_state(&mut self, _bytes: &[u8]) -> PluginResult<()> {
        Ok(())
    }

    fn activate(&mut self, layout: BusLayout, _rate: f64, _frames: usize) -> PluginResult<()> {
        self.activations.fetch_add(1, Ordering::Relaxed);
        self.active = Some(layout);
        Ok(())
    }

    fn deactivate(&mut self) {
        self.active = None;
    }

    fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

impl Plugin<f32> for ChannelFixture {
    fn process(
        &mut self,
        buffer: &mut AudioBuffer<'_, f32>,
        _events: &EventList,
        _context: &mut ProcessContext<'_>,
    ) -> PluginResult<ProcessStatus> {
        assert_eq!(
            buffer.total_input_channels(),
            buffer.total_output_channels()
        );
        for index in 0..buffer.total_input_channels() {
            let sample = buffer.main_inputs()[index][0];
            buffer.main_outputs()[index].fill(sample);
        }
        Ok(ProcessStatus::Continue)
    }
}

fn channel_layout(channels: u32) -> BusLayout {
    use truce_rack_core::bus::{Bus, ChannelConfig};
    let mut layout = BusLayout::new();
    layout
        .inputs
        .push(Bus::main("Input", ChannelConfig::Discrete(channels)));
    layout
        .outputs
        .push(Bus::main("Output", ChannelConfig::Discrete(channels)));
    layout
}

fn fixture(layouts: Vec<BusLayout>) -> ChannelFixture {
    let info = PluginInfo {
        name: "Channel fixture".to_owned(),
        vendor: "MixRelay".to_owned(),
        version: 1,
        category: PluginCategory::Effect,
        path: PathBuf::new(),
        unique_id: "channel-fixture".to_owned(),
        format: "fixture",
        has_editor: false,
        accepts_midi: false,
    };
    ChannelFixture {
        info,
        layouts,
        active: None,
        activations: Arc::new(AtomicUsize::new(0)),
    }
}

fn activate(
    plugin: ChannelFixture,
    index: usize,
    frames: usize,
) -> Result<RackWorkerPlugin, String> {
    RackWorkerPlugin::activate(
        Box::new(plugin),
        index,
        &WorkerActivation {
            editor_allowed: false,
            sample_rate: 48_000.0,
            max_block_size: frames,
        },
        &serde_json::json!({}),
    )
}

fn worker(channels: usize, frames: usize) -> RackWorkerPlugin {
    let layout = channel_layout(u32::try_from(channels).unwrap());
    let mut worker = activate(fixture(vec![layout]), 0, frames).unwrap();
    for (index, channel) in worker.inputs_mut().iter_mut().enumerate() {
        channel.fill(f32::from(u16::try_from(index + 1).unwrap()));
    }
    worker
}

#[test]
fn worker_revalidates_saved_layout_index_before_activation() {
    let scanned = [BusLayout::stereo()];
    let (saved_index, _) = select_effect_layout(&scanned).unwrap();
    for channels in [0, 65, u32::MAX] {
        let plugin = fixture(vec![channel_layout(channels)]);
        let activations = Arc::clone(&plugin.activations);
        assert!(activate(plugin, saved_index, 32).is_err());
        assert_eq!(activations.load(Ordering::Relaxed), 0);
    }
    let plugin = fixture(Vec::new());
    let activations = Arc::clone(&plugin.activations);
    assert!(activate(plugin, saved_index, 32).is_err());
    assert_eq!(activations.load(Ordering::Relaxed), 0);
}

#[test]
fn worker_rejects_over_limit_inputs_outputs_and_bus_sum_overflow() {
    use truce_rack_core::bus::{Bus, ChannelConfig};
    for input_side in [true, false] {
        for counts in [[64, 1], [u32::MAX, 2]] {
            let mut layout = BusLayout::stereo();
            let buses = if input_side {
                &mut layout.inputs
            } else {
                &mut layout.outputs
            };
            buses.clear();
            buses.extend(counts.map(|count| Bus::main("Audio", ChannelConfig::Discrete(count))));
            assert!(select_effect_layout(&[layout.clone()]).is_none());
            let plugin = fixture(vec![layout]);
            let activations = Arc::clone(&plugin.activations);
            assert!(activate(plugin, 0, 32).is_err());
            assert_eq!(activations.load(Ordering::Relaxed), 0);
        }
    }
}

#[test]
fn process_preserves_every_channel_at_supported_boundaries() {
    for channels in [1, 2, 8, 64] {
        let mut worker = worker(channels, 256);
        for frames in [1, 32, 256] {
            worker.clear_outputs();
            worker.process(frames).unwrap();
            for (input, output) in worker.input_storage.iter().zip(&worker.output_storage) {
                assert_eq!(&input[..frames], &output[..frames]);
                assert!(output[frames..].iter().all(|sample| *sample == 0.0));
            }
        }
    }
}

#[test]
fn process_channel_references_allocate_nothing() {
    for channels in [1, 2, 8, 64] {
        let mut worker = worker(channels, 256);
        let allocation = allocation_counter::measure(|| {
            for frames in [1, 32, 256] {
                worker.process(frames).unwrap();
            }
        });
        assert_eq!(allocation.count_total, 0);
    }
}

#[test]
fn process_channel_reference_measurement() {
    const ITERATIONS: u32 = 20_000;
    if std::env::var_os("MIXRELAY_MEASURE_RACK_CHANNELS").is_none() {
        return;
    }
    for channels in [2, 8, 64] {
        for frames in [32, 256] {
            let mut worker = worker(channels, frames);
            for _ in 0..ITERATIONS {
                black_box(&mut worker).process(black_box(frames)).unwrap();
            }
            let mut elapsed = [0.0; 7];
            for ns in &mut elapsed {
                let start = Instant::now();
                for _ in 0..ITERATIONS {
                    black_box(&mut worker).process(black_box(frames)).unwrap();
                }
                *ns = start.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(ITERATIONS);
            }
            elapsed.sort_by(f64::total_cmp);
            for (input, output) in worker.input_storage.iter().zip(&worker.output_storage) {
                assert_eq!(input, output);
            }
            println!(
                "rack-process channels={channels} frames={frames} median_ns={:.3}",
                elapsed[3]
            );
        }
    }
}

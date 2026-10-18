#![allow(clippy::float_cmp, clippy::needless_pass_by_value)] // Exact identity, stereo linkage and timing assertions.
use super::*;
use mixrelay_audio_graph::EffectConfig;

pub(super) fn configs() -> Vec<EffectConfig> {
    [
        (
            "equalizer",
            r#"{"lowDb":6,"midDb":-3,"highDb":3,"lowFrequencyHz":150,"lowQ":0.707,"midFrequencyHz":1000,"midQ":0.707,"highFrequencyHz":6000,"highQ":0.707}"#,
        ),
        ("high_pass", r#"{"frequencyHz":200,"q":0.707}"#),
        ("low_pass", r#"{"frequencyHz":3000,"q":0.707}"#),
        (
            "gate",
            r#"{"thresholdDb":-30,"ratio":4,"attackMs":1,"releaseMs":5,"rangeDb":80}"#,
        ),
        (
            "expander",
            r#"{"thresholdDb":-30,"ratio":2,"attackMs":1,"releaseMs":5,"rangeDb":24}"#,
        ),
        ("channel_utility", r#"{"mode":4}"#),
        ("pan", r#"{"position":0}"#),
        ("balance", r#"{"position":0.5}"#),
        ("delay", r#"{"timeMs":1,"feedback":0.5,"mix":1,"mode":0}"#),
        (
            "ducking",
            r#"{"thresholdDb":-30,"reductionDb":12,"attackMs":0.1,"releaseMs":5}"#,
        ),
        (
            "reverb",
            r#"{"decaySeconds":0.4,"damping":0.5,"mix":1,"roomSize":1,"preDelayMs":0,"algorithm":0}"#,
        ),
        ("chorus", r#"{"rateHz":0.6,"depthMs":3,"mix":1}"#),
        ("delay", r#"{"timeMs":1,"feedback":0.55,"mix":1,"mode":1}"#),
        (
            "reverb",
            r#"{"decaySeconds":4.5,"damping":0.75,"mix":1,"roomSize":2,"preDelayMs":65,"algorithm":1}"#,
        ),
    ]
    .into_iter()
    .map(|(kind, parameters)| {
        serde_json::from_str(&format!(
        r#"{{"version":1,"channels":{},"bypass":false,"effect":{{"kind":"{kind}","parameters":{parameters}}}}}"#,
        if kind == "pan" { 1 } else { 2 }
    )).unwrap()
    })
    .collect()
}

pub(super) fn processor(
    config: EffectConfig,
    revision: u64,
    format: RenderFormat,
) -> Box<BlockProcessor> {
    processor_from_source(config, revision, format, "source")
}

fn processor_from_source(
    config: EffectConfig,
    revision: u64,
    format: RenderFormat,
    source: &str,
) -> Box<BlockProcessor> {
    let graph = GraphDocument::new(
        revision,
        vec![
            node(
                source,
                NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                    selector: DeviceSelector::DefaultInput,
                    output_channels: config.channels,
                    gain_db: 0.0,
                }),
            ),
            node("fx", NodeConfig::Effect(config.clone())),
            node(
                "sink",
                NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                    selector: DeviceSelector::DefaultOutput,
                    input_channels: config.output_channels(),
                }),
            ),
        ],
        vec![
            edge("in", source, AUDIO_PORT_ID, "fx", AUDIO_PORT_ID),
            edge("out", "fx", AUDIO_PORT_ID, "sink", AUDIO_PORT_ID),
        ],
    );
    let bytes = serde_json::to_vec(&graph).unwrap();
    let graph = serde_json::from_slice(&bytes).unwrap();
    let execution = GraphCompiler::default()
        .compile(
            &graph,
            CompileContext {
                format,
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    Box::new(BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap())).unwrap())
}

fn signal(
    config: &EffectConfig,
    input: &[f32],
    control: Option<&[f32]>,
    blocks: &[usize],
) -> Vec<f32> {
    signal_at_rate(config, input, control, blocks, 48_000)
}

fn signal_at_rate(
    config: &EffectConfig,
    input: &[f32],
    control: Option<&[f32]>,
    blocks: &[usize],
    rate: u32,
) -> Vec<f32> {
    let design = effects::Design::new(
        config,
        RenderFormat {
            sample_rate: rate,
            block_frames: 1024,
        },
    )
    .unwrap();
    let mut state = effects::State::new(&design).unwrap();
    let channels = usize::from(config.channels);
    let outputs = usize::from(config.output_channels());
    let frames = input.len() / channels;
    let mut arena = vec![0.0; 1024 * (channels + outputs + 1)];
    let input_slot = PortSlot {
        offset: 0,
        channels: config.channels,
    };
    let output_slot = PortSlot {
        offset: 1024 * channels,
        channels: config.output_channels(),
    };
    let control_slot = PortSlot {
        offset: 1024 * (channels + outputs),
        channels: 1,
    };
    let mut result = Vec::new();
    let mut position = 0;
    let mut block = 0;
    while position < frames {
        let count = blocks[block % blocks.len()].min(frames - position);
        arena[..count * channels]
            .copy_from_slice(&input[position * channels..(position + count) * channels]);
        if let Some(control) = control {
            arena[control_slot.offset..control_slot.offset + count]
                .copy_from_slice(&control[position..position + count]);
        }
        state.process(
            &design,
            &mut arena,
            input_slot,
            output_slot,
            control.map(|_| control_slot),
            count,
        );
        result.extend_from_slice(&arena[output_slot.offset..output_slot.offset + count * outputs]);
        position += count;
        block += 1;
    }
    result
}

#[test]
fn all_effects_are_partition_invariant_finite_and_compile_roundtrip() {
    for config in configs() {
        let channels = usize::from(config.channels);
        let input: Vec<f32> = (0..48_000 * channels)
            .map(|index| if index < channels { 0.5 } else { 0.0 })
            .collect();
        let full = signal(&config, &input, None, &[1024]);
        assert_eq!(full, signal(&config, &input, None, &[1, 7, 128, 31]));
        assert!(
            full.iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 2.0)
        );
        let mut processor = processor(config.clone(), 1, FORMAT);
        let mut output = [0.0; 8];
        processor
            .process_block(
                4,
                &[&input[..4 * channels]],
                &mut [&mut output[..4 * usize::from(config.output_channels())]],
            )
            .unwrap();
        assert_eq!(
            &output[..4 * usize::from(config.output_channels())],
            &full[..4 * usize::from(config.output_channels())]
        );
        let mut bypass = config;
        bypass.bypass = true;
        let dry = signal(&bypass, &input, None, &[128]);
        for (index, sample) in dry.iter().enumerate() {
            assert_eq!(
                *sample,
                input[index / usize::from(bypass.output_channels()) * channels
                    + index % usize::from(bypass.output_channels()).min(channels)]
            );
        }
    }
}

#[test]
fn channel_pair_and_pan_balance_have_expected_geometry_and_levels() {
    let mut config = configs().remove(5);
    for (mode, expected) in [
        [0.8, -0.2],
        [0.3, 0.3],
        [0.8, 0.8],
        [-0.2, -0.2],
        [-0.2, 0.8],
        [-0.8, 0.2],
    ]
    .into_iter()
    .enumerate()
    {
        if let mixrelay_audio_graph::Effect::ChannelUtility(p) = &mut config.effect {
            p.mode = u8::try_from(mode).unwrap();
        }
        let output = signal(&config, &[0.8, -0.2], None, &[1]);
        for (sample, expected) in output.iter().zip(expected) {
            assert!((sample - expected).abs() < 1e-6);
        }
    }
    let pan = signal(&configs()[6], &[1.0], None, &[1]);
    assert!((pan[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((pan[0] * pan[0] + pan[1] * pan[1] - 1.0).abs() < 1e-6);
    assert_eq!(signal(&configs()[7], &[1.0, 1.0], None, &[1]), [0.5, 1.0]);
}

#[test]
fn delay_impulse_has_exact_spacing_feedback_and_zero_time_identity() {
    let mut config = configs().remove(8);
    let mut input = vec![0.0; 512];
    input[0] = 1.0;
    let output = signal(&config, &input, None, &[7]);
    assert_eq!(output[96], 1.0);
    assert_eq!(output[192], 0.5);
    assert_eq!(output[288], 0.25);
    assert_eq!(output[..96], [0.0; 96]);
    if let mixrelay_audio_graph::Effect::Delay(p) = &mut config.effect {
        p.time_ms = 0.0;
    }
    assert_eq!(signal(&config, &input, None, &[32]), input);
}

#[test]
fn ping_pong_impulse_alternates_stereo_pairs_without_block_or_channel_order_dependence() {
    let mut config = configs().remove(12);
    config.channels = 4;
    if let mixrelay_audio_graph::Effect::Delay(p) = &mut config.effect {
        p.feedback = 0.5;
    }
    let mut input = vec![0.0; 240 * 4];
    input[..4].copy_from_slice(&[1.0, 1.0, 0.5, 0.5]);
    let output = signal(&config, &input, None, &[7]);
    assert_eq!(output, signal(&config, &input, None, &[1, 31, 128]));
    for (frame, expected) in [
        (48, [1.0, 0.0, 0.5, 0.0]),
        (96, [0.0, 0.5, 0.0, 0.25]),
        (144, [0.25, 0.0, 0.125, 0.0]),
    ] {
        assert_eq!(&output[frame * 4..frame * 4 + 4], &expected);
    }
    assert!(output[..48 * 4].iter().all(|sample| *sample == 0.0));
    config.bypass = true;
    assert_eq!(signal(&config, &input, None, &[128]), input);
    config.bypass = false;
    config.channels = 1;
    let mut mono = vec![0.0; 200];
    mono[0] = 1.0;
    let output = signal(&config, &mono, None, &[31]);
    assert_eq!(output[48], 1.0);
    assert_eq!(output[96], 0.5);
    if let mixrelay_audio_graph::Effect::Delay(p) = &mut config.effect {
        p.time_ms = 0.0;
    }
    assert_eq!(signal(&config, &mono, None, &[31]), mono);
}

#[test]
fn reverb_pre_delay_moves_the_entire_wet_response_by_the_requested_sample_count() {
    for rate in [44_100, 48_000, 96_000] {
        let mut config = configs().remove(10);
        let frames = usize::try_from(rate).unwrap() / 4;
        let mut input = vec![0.0; frames * 2];
        input[..2].fill(1.0);
        let immediate = signal_at_rate(&config, &input, None, &[128], rate);
        if let mixrelay_audio_graph::Effect::Reverb(p) = &mut config.effect {
            p.pre_delay_ms = 20.0;
        }
        let deferred = signal_at_rate(&config, &input, None, &[31, 1, 7], rate);
        let shift = usize::try_from(rate / 50).unwrap() * 2;
        assert!(deferred[..shift].iter().all(|sample| *sample == 0.0));
        assert_eq!(&deferred[shift..], &immediate[..immediate.len() - shift]);
    }
}

#[test]
fn dense_algorithm_produces_a_different_more_diffuse_tail_than_classic() {
    let mut classic = configs().remove(10);
    classic.channels = 1;
    let mut dense = classic.clone();
    if let mixrelay_audio_graph::Effect::Reverb(p) = &mut dense.effect {
        p.algorithm = 1;
    }
    let mut input = vec![0.0; 24_000];
    input[0] = 1.0;
    let sparse = signal(&classic, &input, None, &[128]);
    let diffuse = signal(&dense, &input, None, &[128]);
    assert_ne!(sparse, diffuse);
    let density = |samples: &[f32]| samples.iter().filter(|sample| sample.abs() > 1e-7).count();
    assert!(density(&diffuse[4_800..14_400]) > density(&sparse[4_800..14_400]));
    assert_eq!(diffuse, signal(&dense, &input, None, &[1, 7, 31]));
}

#[test]
fn reverb_damping_reduces_high_frequency_tail_energy_and_maximum_feedback_remains_bounded() {
    let mut config = configs().remove(13);
    config.channels = 1;
    if let mixrelay_audio_graph::Effect::Reverb(p) = &mut config.effect {
        p.pre_delay_ms = 0.0;
        p.room_size = 1.0;
        p.damping = 0.0;
    }
    let mut input = vec![0.0; 48_000];
    input[0] = 0.5;
    let bright = signal(&config, &input, None, &[128]);
    if let mixrelay_audio_graph::Effect::Reverb(p) = &mut config.effect {
        p.damping = 0.95;
    }
    let dark = signal(&config, &input, None, &[128]);
    let roughness = |samples: &[f32]| {
        let power = samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>();
        samples
            .windows(2)
            .map(|pair| f64::from(pair[1] - pair[0]).powi(2))
            .sum::<f64>()
            / power
    };
    assert!(roughness(&dark[12_000..]) < roughness(&bright[12_000..]) * 0.5);
    if let mixrelay_audio_graph::Effect::Reverb(p) = &mut config.effect {
        p.pre_delay_ms = 200.0;
        p.room_size = 2.0;
        p.decay_seconds = 6.0;
    }
    assert!(
        signal(&config, &input, None, &[31])
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() < 2.0)
    );
    config = configs().remove(12);
    config.channels = 1;
    if let mixrelay_audio_graph::Effect::Delay(p) = &mut config.effect {
        p.feedback = 0.9;
    }
    let echo = signal(&config, &input, None, &[31]);
    assert_eq!(echo[48], 0.5);
    assert!(echo[48 * 100].abs() < 0.0001);
    assert!(
        echo.iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 0.5)
    );
}

#[test]
fn room_and_tunnel_have_different_onsets_decay_tails_and_stereo_diffusion() {
    let mut room = configs().remove(10);
    if let mixrelay_audio_graph::Effect::Reverb(p) = &mut room.effect {
        p.room_size = 0.6;
        p.decay_seconds = 0.4;
        p.pre_delay_ms = 4.0;
        p.damping = 0.6;
        p.algorithm = 1;
    }
    let tunnel = configs().remove(13);
    let mut input = vec![0.0; 96_000 * 2];
    input[..2].fill(1.0);
    let small = signal(&room, &input, None, &[128]);
    let large = signal(&tunnel, &input, None, &[128]);
    let onset = |samples: &[f32]| {
        samples
            .iter()
            .position(|sample| sample.abs() > 1e-6)
            .unwrap()
            / 2
    };
    assert!(onset(&large) > onset(&small) + 3000);
    let energy = |samples: &[f32]| {
        samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
    };
    assert!(energy(&large[48_000 * 2..]) > energy(&small[48_000 * 2..]) * 100.0);
    assert!(
        large
            .chunks_exact(2)
            .any(|pair| (pair[0] - pair[1]).abs() > 0.001)
    );
    assert!(
        large
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() < 2.0)
    );
    let mut left_only = vec![0.0; 12_000 * 2];
    left_only[0] = 1.0;
    let spread = signal(&room, &left_only, None, &[31]);
    assert!(spread.chunks_exact(2).any(|pair| pair[1].abs() > 0.001));
    room.channels = 1;
    let mut mono = vec![0.0; 12_000];
    mono[0] = 1.0;
    let output = signal(&room, &mono, None, &[31]);
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert!(output.iter().all(|sample| sample.is_finite()));
}

#[test]
fn spatial_preset_edits_reuse_history_and_do_not_allocate_or_destroy_in_the_callback() {
    for index in [12, 13] {
        let config = configs().remove(index);
        let mut current = processor(config.clone(), 1, FORMAT);
        let input = [0.25; 8];
        let mut output = [0.0; 8];
        current
            .process_block(4, &[&input], &mut [&mut output])
            .unwrap();
        let mut changed = config;
        match &mut changed.effect {
            mixrelay_audio_graph::Effect::Delay(p) => {
                p.mode = 0;
                p.time_ms = 1.5;
            }
            mixrelay_audio_graph::Effect::Reverb(p) => {
                p.room_size = 0.5;
                p.pre_delay_ms = 0.0;
                p.decay_seconds = 0.4;
                p.algorithm = 0;
            }
            _ => unreachable!(),
        }
        let replacement = processor(changed, 2, FORMAT);
        let exchange = crate::ProcessorExchange::new(1);
        exchange.submit(replacement).unwrap();
        HEAP_OPERATIONS.with(|counter| counter.set(Some(0)));
        assert!(exchange.activate_pending(&mut current));
        let mut retained_tail = false;
        for _ in 0..1000 {
            current
                .process_block(4, &[&[0.0; 8]], &mut [&mut output])
                .unwrap();
            retained_tail |= output.iter().any(|sample| sample.abs() > 1e-8);
        }
        assert!(
            retained_tail,
            "parameter activation must retain spatial history"
        );
        current.effect_states[0].suspend();
        for _ in 0..1000 {
            current
                .process_block(4, &[&[0.0; 8]], &mut [&mut output])
                .unwrap();
            assert_eq!(output, [0.0; 8]);
        }
        assert_eq!(
            HEAP_OPERATIONS.with(|counter| counter.replace(None)),
            Some(0)
        );
        exchange.reclaim_retired();
    }
}

#[test]
fn ducking_requires_control_input_and_recovers_without_it() {
    let config = configs().remove(9);
    let input = vec![0.5; 9600];
    assert_eq!(signal(&config, &input, None, &[128]), input);
    let mut control = vec![1.0; 4800];
    control[1000..].fill(0.0);
    let output = signal(&config, &input, Some(&control), &[31]);
    assert!((output[1000] - 0.5 * 10.0_f32.powf(-12.0 / 20.0)).abs() < 1e-5);
    assert!(output[9500] > 0.499);
    assert!(output.chunks_exact(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn gate_and_expander_attenuate_quiet_signal_and_open_for_loud_signal() {
    for index in [3, 4] {
        let config = &configs()[index];
        let quiet = signal(config, &vec![0.001; 9600], None, &[64]);
        assert!(quiet[9500] < 0.0001);
        let loud = signal(config, &vec![0.5; 9600], None, &[64]);
        assert!(loud[9500] > 0.499);
    }
}

#[test]
fn effect_activation_retains_echo_and_off_invalidates_old_history_without_allocation() {
    let config = configs().remove(8);
    let mut current = processor(config.clone(), 1, FORMAT);
    let input = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let mut output = [0.0; 8];
    current
        .process_block(4, &[&input], &mut [&mut output])
        .unwrap();
    let exchange = crate::ProcessorExchange::new(1);
    exchange.submit(processor(config, 2, FORMAT)).unwrap();
    HEAP_OPERATIONS.with(|counter| counter.set(Some(0)));
    assert!(exchange.activate_pending(&mut current));
    for _ in 0..11 {
        current
            .process_block(4, &[&[0.0; 8]], &mut [&mut output])
            .unwrap();
    }
    current
        .process_block(4, &[&[0.0; 8]], &mut [&mut output])
        .unwrap();
    assert_eq!(output[0], 1.0);
    current.effect_states[0].suspend();
    for _ in 0..30 {
        current
            .process_block(4, &[&[0.0; 8]], &mut [&mut output])
            .unwrap();
        assert_eq!(output, [0.0; 8]);
    }
    assert_eq!(
        HEAP_OPERATIONS.with(|counter| counter.replace(None)),
        Some(0)
    );
    exchange.reclaim_retired();
}

#[test]
fn reconnecting_effect_input_discards_previous_source_tail_without_callback_allocation() {
    let config = configs().remove(8);
    let mut current = processor_from_source(config.clone(), 1, FORMAT, "source-a");
    let mut output = [0.0; 8];
    current
        .process_block(
            4,
            &[&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]],
            &mut [&mut output],
        )
        .unwrap();
    let exchange = crate::ProcessorExchange::new(1);
    exchange
        .submit(processor_from_source(config, 2, FORMAT, "source-b"))
        .unwrap();
    HEAP_OPERATIONS.with(|counter| counter.set(Some(0)));
    assert!(exchange.activate_pending(&mut current));
    for _ in 0..32 {
        current
            .process_block(4, &[&[0.0; 8]], &mut [&mut output])
            .unwrap();
        assert_eq!(output, [0.0; 8]);
    }
    assert_eq!(
        HEAP_OPERATIONS.with(|counter| counter.replace(None)),
        Some(0)
    );
    exchange.reclaim_retired();
}

#[test]
fn invalid_configuration_and_aggregate_capacity_are_rejected() {
    let mut config = configs().remove(8);
    if let mixrelay_audio_graph::Effect::Delay(p) = &mut config.effect {
        p.feedback = f32::NAN;
    }
    assert!(config.validate().is_err());
    config = configs().remove(8);
    config.channels = 64;
    assert!(
        effects::Design::new(
            &config,
            RenderFormat {
                sample_rate: 384_000,
                block_frames: 128
            }
        )
        .is_err()
    );
    let mut pan = configs().remove(6);
    pan.channels = 2;
    assert!(pan.validate().is_err());
    let malformed = r#"{"version":1,"channels":2,"bypass":false,"effect":{"kind":"delay","parameters":{"timeMs":1,"feedback":0,"mix":1,"mode":0,"unexpected":true}}}"#;
    assert!(serde_json::from_str::<EffectConfig>(malformed).is_err());
}

#[test]
fn filters_reject_out_of_band_and_equalizer_has_expected_center_gain() {
    let amplitude = |config: &EffectConfig, hz: f32| {
        #[allow(clippy::cast_precision_loss)]
        let input: Vec<f32> = (0..48_000)
            .flat_map(|frame| {
                let value = (std::f32::consts::TAU * hz * frame as f32 / 48_000.0).sin() * 0.1;
                [value, value]
            })
            .collect();
        let output = signal(config, &input, None, &[128]);
        let power = output[48_000..]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>();
        (power / 48_000.0).sqrt() / (0.1 / 2.0_f64.sqrt())
    };
    let all = configs();
    assert!(amplitude(&all[1], 20.0) < 0.02);
    assert!(amplitude(&all[1], 2000.0) > 0.99);
    assert!(amplitude(&all[2], 15000.0) < 0.04);
    assert!(amplitude(&all[2], 100.0) > 0.99);
    let mut eq = all[0].clone();
    if let mixrelay_audio_graph::Effect::Equalizer(p) = &mut eq.effect {
        p.low_db = 0.0;
        p.high_db = 0.0;
        p.mid_db = 6.0;
    }
    assert!((amplitude(&eq, 1000.0) - 10.0_f64.powf(6.0 / 20.0)).abs() < 0.002);
}

#[test]
fn every_effect_callback_and_activation_is_allocation_free_across_formats() {
    for rate in [44_100, 48_000, 96_000, 192_000] {
        let format = RenderFormat {
            sample_rate: rate,
            block_frames: 128,
        };
        for config in configs() {
            let mut current = processor(config.clone(), 1, format);
            let exchange = crate::ProcessorExchange::new(1);
            exchange
                .submit(processor(config.clone(), 2, format))
                .unwrap();
            let input = vec![0.25; 128 * usize::from(config.channels)];
            let mut output = vec![0.0; 128 * usize::from(config.output_channels())];
            HEAP_OPERATIONS.with(|counter| counter.set(Some(0)));
            assert!(exchange.activate_pending(&mut current));
            for _ in 0..32 {
                current
                    .process_block(128, &[&input], &mut [&mut output])
                    .unwrap();
            }
            assert_eq!(
                HEAP_OPERATIONS.with(|counter| counter.replace(None)),
                Some(0)
            );
            assert!(output.iter().all(|v| v.is_finite()));
            exchange.reclaim_retired();
        }
    }
}

#[test]
fn parameter_edit_smooths_pan_and_bypass_without_resetting_audio_history() {
    let mut config = configs().remove(6);
    if let mixrelay_audio_graph::Effect::Pan(p) = &mut config.effect {
        p.position = -1.0;
    }
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 128,
    };
    let mut current = processor(config.clone(), 1, format);
    let input = [0.5; 128];
    let mut output = [0.0; 256];
    current
        .process_block(128, &[&input], &mut [&mut output])
        .unwrap();
    let before = output[254];
    if let mixrelay_audio_graph::Effect::Pan(p) = &mut config.effect {
        p.position = 1.0;
    }
    let exchange = crate::ProcessorExchange::new(1);
    exchange
        .submit(processor(config.clone(), 2, format))
        .unwrap();
    assert!(exchange.activate_pending(&mut current));
    current
        .process_block(128, &[&input], &mut [&mut output])
        .unwrap();
    assert!((output[0] - before).abs() < 0.003);
    for _ in 0..32 {
        current
            .process_block(128, &[&input], &mut [&mut output])
            .unwrap();
    }
    assert!(output[254].abs() < 1e-6);
    assert!((output[255] - 0.5).abs() < 1e-6);
    exchange.reclaim_retired();
    config.bypass = true;
    exchange.submit(processor(config, 3, format)).unwrap();
    assert!(exchange.activate_pending(&mut current));
    current
        .process_block(128, &[&input], &mut [&mut output])
        .unwrap();
    assert!(output[0] < 0.003);
    for _ in 0..40 {
        current
            .process_block(128, &[&input], &mut [&mut output])
            .unwrap();
    }
    assert_eq!(output, [0.5; 256]);
    exchange.reclaim_retired();
}

#[test]
fn mute_keeps_delay_processing_while_off_blocks_input_and_discards_history() {
    for enabled in [false, true] {
        let config = configs().remove(8);
        let mut current = processor(config, 1, FORMAT);
        let exchange = crate::ProcessorExchange::new(1);
        exchange
            .publish_controls(
                current.plan().controls(),
                2,
                &[crate::RuntimeControlUpdate {
                    node_id: NodeId::from("fx"),
                    control: mixrelay_audio_graph::NodeControl {
                        enabled,
                        muted: true,
                    },
                    gain_db: None,
                }],
            )
            .unwrap();
        let mut output = [0.0; 8];
        current
            .process_block(
                4,
                &[&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]],
                &mut [&mut output],
            )
            .unwrap();
        assert_eq!(output, [0.0; 8]);
        exchange
            .publish_controls(
                current.plan().controls(),
                3,
                &[crate::RuntimeControlUpdate {
                    node_id: NodeId::from("fx"),
                    control: mixrelay_audio_graph::NodeControl::default(),
                    gain_db: None,
                }],
            )
            .unwrap();
        HEAP_OPERATIONS.with(|counter| counter.set(Some(0)));
        for _ in 0..11 {
            current
                .process_block(4, &[&[0.0; 8]], &mut [&mut output])
                .unwrap();
        }
        current
            .process_block(4, &[&[0.0; 8]], &mut [&mut output])
            .unwrap();
        assert_eq!(output[0], if enabled { 1.0 } else { 0.0 });
        assert_eq!(
            HEAP_OPERATIONS.with(|counter| counter.replace(None)),
            Some(0)
        );
    }
}

#[test]
fn equalizer_low_and_high_bands_follow_their_frequency_and_q() {
    let amplitude = |config: &EffectConfig, hz: f32| {
        #[allow(clippy::cast_precision_loss)]
        let input: Vec<f32> = (0..48_000)
            .flat_map(|frame| {
                let value = (std::f32::consts::TAU * hz * frame as f32 / 48_000.0).sin() * 0.01;
                [value, value]
            })
            .collect();
        let output = signal(config, &input, None, &[127, 511, 64]);
        (output[48_000..]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            / 48_000.0)
            .sqrt()
            / (0.01 / 2.0_f64.sqrt())
    };
    for low in [true, false] {
        let mut config = configs()[0].clone();
        let center = if low { 400.0 } else { 4_000.0 };
        let mixrelay_audio_graph::Effect::Equalizer(p) = &mut config.effect else {
            unreachable!()
        };
        p.low_db = if low { 12.0 } else { 0.0 };
        p.mid_db = 0.0;
        p.high_db = if low { 0.0 } else { 12.0 };
        p.low_frequency_hz = center;
        p.high_frequency_hz = center;
        p.low_q = 4.0;
        p.high_q = 4.0;
        let expected = 10.0_f64.powf(12.0 / 20.0);
        assert!((amplitude(&config, center) - expected).abs() < 0.002);
        let narrow = amplitude(&config, center * 1.5);
        assert!(narrow < 1.5, "narrow band response: {narrow}");
        let mixrelay_audio_graph::Effect::Equalizer(p) = &mut config.effect else {
            unreachable!()
        };
        if low {
            p.low_q = 0.2;
        } else {
            p.high_q = 0.2;
        }
        let wide = amplitude(&config, center * 1.5);
        assert!(
            wide > narrow * 2.0,
            "Q changes width: narrow {narrow}, wide {wide}"
        );
        let mixrelay_audio_graph::Effect::Equalizer(p) = &mut config.effect else {
            unreachable!()
        };
        if low {
            p.low_frequency_hz = center * 3.0;
            p.low_q = 4.0;
        } else {
            p.high_frequency_hz = center * 3.0;
            p.high_q = 4.0;
        }
        assert!(
            amplitude(&config, center) < 1.15,
            "moving the center moves the actual bell"
        );
        assert!((amplitude(&config, center * 3.0) - expected).abs() < 0.002);
        // Bell bands return to unity at DC; they must not behave as fixed shelves.
        assert!(amplitude(&config, 20.0) < 1.01);
    }
}

#[test]
fn changing_all_equalizer_band_parameters_activates_without_callback_heap_operations() {
    let format = RenderFormat {
        sample_rate: 48_000,
        block_frames: 128,
    };
    let config = configs()[0].clone();
    let mut changed = config.clone();
    let mixrelay_audio_graph::Effect::Equalizer(p) = &mut changed.effect else {
        unreachable!()
    };
    p.low_frequency_hz = 330.0;
    p.low_q = 3.0;
    p.low_db = -12.0;
    p.mid_frequency_hz = 2_000.0;
    p.mid_q = 4.0;
    p.mid_db = 6.0;
    p.high_frequency_hz = 8_000.0;
    p.high_q = 0.3;
    p.high_db = 12.0;
    let mut current = processor(config, 1, format);
    let exchange = crate::ProcessorExchange::new(1);
    exchange.submit(processor(changed, 2, format)).unwrap();
    let input = [0.01; 256];
    let mut output = [0.0; 256];
    HEAP_OPERATIONS.with(|counter| counter.set(Some(0)));
    assert!(exchange.activate_pending(&mut current));
    for _ in 0..64 {
        current
            .process_block(128, &[&input], &mut [&mut output])
            .unwrap();
    }
    assert_eq!(
        HEAP_OPERATIONS.with(|counter| counter.replace(None)),
        Some(0)
    );
    assert!(output.iter().all(|sample| sample.is_finite()));
}

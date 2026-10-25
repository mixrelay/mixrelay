use super::*;

fn mixer(render: u32, capture: u32) -> ClientMix {
    let mut mixer = ClientMix::new(
        Geometry {
            endpoint_id: [1; 16],
            render_channels: render,
            capture_channels: capture,
        },
        16_384,
    )
    .unwrap();
    for id in [42, 43, 44] {
        assert!(mixer.register(id));
    }
    mixer
}

#[test]
fn a_prepared_client_output_survives_missing_postmix_cycles_and_different_capture_widths() {
    let mixer = mixer(2, 2);
    let source: Vec<_> = (0..1024_u16)
        .map(|value| f32::from(value + 1) / 2048.0)
        .collect();
    // The observed failed host sequence omitted postmix [167020,167396).
    // This client supplied all of [166884,167396) before any input requested it.
    assert!(mixer.render(42, 166_884, &source, 512).accepted);
    let mut short = [0.0; 256];
    assert_eq!(mixer.capture(166_884, &mut short, 128).available, 128);
    assert_eq!(
        short.map(f32::to_bits),
        source[..256]
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
            .as_slice()
    );
    let mut full = [0.0; 1024];
    assert_eq!(mixer.capture(166_884, &mut full, 512).available, 512);
    assert_eq!(
        full.map(f32::to_bits),
        source
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
            .as_slice()
    );
}

#[test]
fn overlapping_clients_mix_once_and_registration_reuse_cannot_replay_or_reset_its_peer() {
    let mut mixer = mixer(2, 4);
    assert!(mixer.render(42, 100, &[0.125; 16], 8).accepted);
    assert!(mixer.render(43, 100, &[0.25; 16], 8).accepted);
    let mut output = [0.0; 32];
    assert_eq!(mixer.capture(100, &mut output, 8).available, 8);
    assert!(
        output
            .chunks_exact(4)
            .all(|frame| frame == [0.375, 0.375, 0.0, 0.0])
    );
    mixer.remove(43);
    assert!(mixer.register(43));
    assert_eq!(mixer.capture(100, &mut output, 8).available, 8);
    assert!(
        output
            .chunks_exact(4)
            .all(|frame| frame == [0.125, 0.125, 0.0, 0.0])
    );
    assert!(mixer.render(43, 100, &[0.5; 16], 8).accepted);
    assert_eq!(mixer.capture(100, &mut output, 8).available, 8);
    assert!(
        output
            .chunks_exact(4)
            .all(|frame| frame == [0.625, 0.625, 0.0, 0.0])
    );
    mixer.remove(42);
    assert_eq!(mixer.capture(100, &mut output, 8).available, 8);
    assert!(
        output
            .chunks_exact(4)
            .all(|frame| frame == [0.5, 0.5, 0.0, 0.0])
    );
}

#[test]
fn a_finished_client_supplies_zero_beyond_its_window_while_its_peer_continues() {
    let mixer = mixer(2, 2);
    assert!(mixer.render(42, 0, &[0.125; 16], 8).accepted);
    assert!(mixer.render(43, 0, &[0.25; 8], 4).accepted);
    let mut output = [9.0; 16];
    let outcome = mixer.capture(0, &mut output, 8);
    assert!(outcome.accepted);
    assert_eq!(outcome.available, 8);
    assert_eq!(
        output.map(f32::to_bits),
        [
            0.375, 0.375, 0.375, 0.375, 0.375, 0.375, 0.375, 0.375, 0.125, 0.125, 0.125, 0.125,
            0.125, 0.125, 0.125, 0.125
        ]
        .map(f32::to_bits)
    );
    assert!(!mixer.render(999, 0, &[0.25; 16], 8).accepted);
    assert!(!mixer.render(42, 4, &[0.25; 16], 8).accepted);
    assert_eq!(mixer.capture(u64::MAX, &mut output, 8).available, 0);
    assert_eq!(output.map(f32::to_bits), [0; 16]);
}

#[test]
fn replacement_preserves_registration_but_starts_an_empty_pcm_timeline() {
    let mixer = mixer(2, 2);
    assert!(mixer.render(42, 0, &[0.5; 16], 8).accepted);
    let replacement = mixer.replacement(mixer.geometry(), 16_384).unwrap();
    let mut output = [1.0; 16];
    assert_eq!(replacement.capture(0, &mut output, 8).available, 0);
    assert_eq!(output.map(f32::to_bits), [0; 16]);
    assert!(replacement.render(42, 0, &[0.25; 16], 8).accepted);
    assert_eq!(replacement.capture(0, &mut output, 8).available, 8);
    assert_eq!(output.map(f32::to_bits), [0.25_f32.to_bits(); 16]);
}

#[test]
fn single_and_multiple_client_callback_transfers_never_allocate_or_free() {
    for (capture, contributors) in [(2, 1), (2, 2), (4, 1), (4, 2)] {
        let mixer = mixer(2, capture);
        let mut captured = [0.0; 512];
        let usage = allocation_counter::measure(|| {
            for cycle in 0..100_u64 {
                assert!(mixer.render(42, cycle * 128, &[0.125; 256], 128).accepted);
                if contributors == 2 {
                    assert!(mixer.render(43, cycle * 128, &[0.25; 256], 128).accepted);
                }
                assert_eq!(
                    mixer
                        .capture(cycle * 128, &mut captured[..capture as usize * 128], 128)
                        .available,
                    128
                );
            }
        });
        assert_eq!(usage.count_total, 0);
        assert_eq!(usage.bytes_total, 0);
        assert_eq!(usage.count_current, 0);
    }
}

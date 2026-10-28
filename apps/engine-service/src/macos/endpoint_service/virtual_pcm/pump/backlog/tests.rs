use super::*;

fn clock(epoch: u64) -> ClientPcmClock {
    ClientPcmClock {
        incarnation: [7; 16],
        epoch,
        anchor: 10_000,
        rate: 48_000,
        running: true,
        first: Some(0),
        through: 0,
    }
}

fn window(clock: ClientPcmClock, first: u64, discontinuity: bool) -> MixWindow {
    MixWindow {
        clock,
        first,
        frames: 128,
        discontinuity,
    }
}

/// Stereo frames carry their native coordinate, so every copy shows where it came from.
#[expect(
    clippy::cast_precision_loss,
    reason = "fixture coordinates stay below 2^24"
)]
fn pcm(first: u64) -> [f32; 256] {
    std::array::from_fn(|index| (first + index as u64 / 2) as f32)
}

/// Pushes `count` continuous windows from `first`; the first may open a discontinuity.
fn retain(backlog: &mut Backlog, clock: ClientPcmClock, first: u64, count: u64, gap: bool) {
    for block in 0..count {
        let frame = first + block * 128;
        backlog.push(window(clock, frame, gap && block == 0), &pcm(frame));
    }
}

/// Converts everything retained, returning each chunk's coordinates and restart flag.
fn drain(backlog: &mut Backlog) -> Vec<(u64, u64, bool)> {
    let mut runs: Vec<(u64, u64, bool)> = Vec::new();
    let mut output = [0.0; 256];
    while let Some(chunk) = backlog.peek(128) {
        let output = &mut output[..chunk.frames * 2];
        backlog.copy(output);
        assert_eq!(output, &pcm(chunk.first)[..chunk.frames * 2]);
        match runs.last_mut() {
            Some((epoch, end, _)) if !chunk.restart && *epoch == chunk.clock.epoch => {
                assert_eq!(*end, chunk.first, "a continuing run skipped frames");
                *end += chunk.frames as u64;
            }
            _ => runs.push((
                chunk.clock.epoch,
                chunk.first + chunk.frames as u64,
                chunk.restart,
            )),
        }
        if chunk.restart {
            backlog.restarted();
        }
        backlog.consume(chunk.frames);
    }
    runs
}

#[test]
fn a_gap_or_a_new_timeline_keeps_every_earlier_retained_run() {
    let mut backlog = Backlog::new(2).unwrap();
    retain(&mut backlog, clock(2), 0, 4, true);
    // Same timeline after a skipped interval, then a restarted native timeline.
    retain(&mut backlog, clock(2), 5_000, 2, true);
    retain(&mut backlog, clock(3), 0, 3, true);
    let starts: Vec<_> = std::iter::from_fn(|| {
        let chunk = backlog.peek(1_024)?;
        backlog.consume(chunk.frames);
        Some((chunk.clock.epoch, chunk.first, chunk.frames, chunk.restart))
    })
    .collect();
    assert_eq!(
        starts,
        [(2, 0, 512, true), (2, 5_000, 256, true), (3, 0, 384, true)]
    );
    assert_eq!(backlog.dropped_frames(), 0);
}

#[test]
fn retained_pcm_continues_one_stream_after_the_store_empties() {
    let mut backlog = Backlog::new(2).unwrap();
    retain(&mut backlog, clock(2), 0, 2, true);
    assert_eq!(drain(&mut backlog), [(2, 256, true)]);
    retain(&mut backlog, clock(2), 256, 2, false);
    assert_eq!(drain(&mut backlog), [(2, 512, false)]);
    // A stopped timeline ends its run; the next window starts a new stream.
    retain(&mut backlog, clock(2), 512, 1, false);
    assert_eq!(backlog.clear(), 128);
    retain(&mut backlog, clock(2), 640, 1, false);
    assert_eq!(drain(&mut backlog), [(2, 768, true)]);
}

#[test]
fn full_retention_drops_only_the_oldest_frames_and_restarts_after_them() {
    let mut backlog = Backlog::new(2).unwrap();
    // Wrap the ring before the overflow so both copies cross its end.
    retain(&mut backlog, clock(2), 0, 3, true);
    assert_eq!(drain(&mut backlog), [(2, 384, true)]);
    retain(&mut backlog, clock(3), 0, 4, true);
    retain(&mut backlog, clock(4), 0, FRAMES / 128 + 2, true);
    // Run 3 and the first 256 frames of run 4 left retention, nothing else.
    assert_eq!(backlog.dropped_frames(), 512 + 256);
    assert_eq!(backlog.frames(), FRAMES);
    assert_eq!(drain(&mut backlog), [(4, FRAMES + 256, true)]);
}

#[test]
fn a_full_run_table_drops_the_oldest_run_whole() {
    let mut backlog = Backlog::new(2).unwrap();
    for epoch in 0..=MAX_RUNS as u64 {
        retain(&mut backlog, clock(epoch + 2), 0, 2, true);
    }
    assert_eq!(backlog.dropped_frames(), 256);
    let runs = drain(&mut backlog);
    assert_eq!(runs.len(), MAX_RUNS);
    assert_eq!(runs[0], (3, 256, true));
}

#[test]
fn the_whole_store_is_resident_when_control_prepares_it() {
    let backlog = Backlog::new(2).unwrap();
    // SAFETY: sysconf only reads a system constant.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
    let start = backlog.samples.as_ptr() as usize;
    let first = start / page * page;
    let bytes = start + backlog.samples.len() * size_of::<f32>() - first;
    let mut resident = vec![0_u8; bytes.div_ceil(page)];
    // SAFETY: the range covers pages of this live allocation and the vector has one
    // byte per page. Residency is read immediately after preparation, before any
    // memory pressure could evict pages the RT path would otherwise fault in.
    let status = unsafe {
        libc::mincore(
            first as *const libc::c_void,
            bytes,
            resident.as_mut_ptr().cast(),
        )
    };
    assert_eq!(status, 0);
    assert!(
        resident.iter().all(|page| page & 1 != 0),
        "a lazily zeroed store leaves first-touch faults to the RT pump"
    );
}

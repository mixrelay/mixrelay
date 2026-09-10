use super::*;
use crate::ProcessorExchange;
use std::sync::atomic::{AtomicUsize, Ordering};
use tonenari_audio_graph::{CompileContext, CompileLimits, GraphCompiler, GraphDocument};

struct Counter {
    identity: u64,
    cursor: usize,
    placeholder: bool,
    drops: Arc<AtomicUsize>,
}

impl HostedAudioProcessor for Counter {
    fn process(&mut self, _input: &[f32], output: &mut [f32], _frames: usize) -> bool {
        self.cursor += 1;
        output[0] = u16::try_from(self.cursor).map(f32::from).unwrap();
        !self.placeholder
    }
    fn reuse_identity(&self) -> Option<u64> {
        Some(self.identity)
    }
    fn requests_reuse(&self) -> bool {
        self.placeholder
    }
}
impl Drop for Counter {
    fn drop(&mut self) {
        if !self.placeholder {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn processor(revision: u64, placeholder: bool, drops: &Arc<AtomicUsize>) -> Box<BlockProcessor> {
    let execution = GraphCompiler::default()
        .compile(
            &GraphDocument::new(revision, vec![], vec![]),
            CompileContext {
                format: RenderFormat {
                    sample_rate: 48000,
                    block_frames: 32,
                },
                limits: CompileLimits::default(),
            },
        )
        .unwrap();
    let mut processor = BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()));
    processor.hosted_processors = vec![Box::new(Counter {
        identity: 7,
        cursor: 0,
        placeholder,
        drops: Arc::clone(drops),
    }) as Box<dyn HostedAudioProcessor>]
    .into_boxed_slice();
    processor.hosted_identities = Box::new([(7, 0)]);
    Box::new(processor)
}

#[test]
fn activation_moves_the_complete_instance_without_resetting_pcm_cursor() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut current = processor(1, false, &drops);
    let exchange = ProcessorExchange::new(1);
    let mut output = [0.0];
    assert!(current.hosted_processors[0].process(&[], &mut output, 1));
    exchange.submit(processor(2, true, &drops)).unwrap();
    assert!(
        exchange
            .activate_pending_configured(&mut current, |_| Ok::<_, ()>(()))
            .unwrap()
    );
    exchange.reclaim_retired();
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert!(current.hosted_processors[0].process(&[], &mut output, 1));
    assert_eq!(output[0].to_bits(), 2.0_f32.to_bits());
    drop(current);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn rejected_configuration_and_cancel_do_not_take_the_active_instance() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut current = processor(1, false, &drops);
    let exchange = ProcessorExchange::new(1);
    exchange.submit(processor(2, true, &drops)).unwrap();
    assert_eq!(
        exchange.activate_pending_configured(&mut current, |_| Err("rejected")),
        Err("rejected")
    );
    assert!(!current.hosted_processors[0].requests_reuse());
    drop(exchange);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert!(current.hosted_processors[0].process(&[], &mut [0.0], 1));
}

#[test]
fn missing_identity_never_activates_a_placeholder() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut current = processor(1, false, &drops);
    current.hosted_identities = Box::new([(8, 0)]);
    let exchange = ProcessorExchange::new(1);
    exchange.submit(processor(2, true, &drops)).unwrap();
    assert!(
        !exchange
            .activate_pending_configured(&mut current, |_| -> Result<(), ()> {
                panic!("native activation must not run without the required owner")
            })
            .unwrap()
    );
    assert_eq!(exchange.revisions().active, 1);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
}

#[test]
fn removal_marks_the_old_owner_inactive_before_retired_reclamation() {
    struct OwnerFlag(Arc<std::sync::atomic::AtomicBool>);
    impl HostedAudioProcessor for OwnerFlag {
        fn process(&mut self, _: &[f32], _: &mut [f32], _: usize) -> bool {
            true
        }
        fn set_active(&mut self, active: bool) {
            self.0.store(active, Ordering::Release);
        }
    }
    let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut current = processor(1, false, &drops);
    current.hosted_processors =
        vec![Box::new(OwnerFlag(Arc::clone(&active))) as Box<dyn HostedAudioProcessor>]
            .into_boxed_slice();
    current.hosted_identities = Box::new([]);
    let mut removed = processor(2, true, &drops);
    removed.hosted_processors = Box::new([]);
    removed.hosted_identities = Box::new([]);
    let exchange = ProcessorExchange::new(1);
    exchange.submit(removed).unwrap();
    assert!(
        exchange
            .activate_pending_configured(&mut current, |_| Ok::<_, ()>(()))
            .unwrap()
    );
    // The retired holder still owns the flag; it is no longer a reuse candidate.
    assert_eq!(Arc::strong_count(&active), 2);
    assert!(!active.load(Ordering::Acquire));
    exchange.reclaim_retired();
    assert_eq!(Arc::strong_count(&active), 1);
}

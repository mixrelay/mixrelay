use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use arc_swap::{ArcSwap, Guard};
use parking_lot::Mutex;
use thiserror::Error;
use tonenari_audio_graph::ExecutionPlan;

pub struct PlanStore {
    active: ArcSwap<ExecutionPlan>,
    writer: Mutex<()>,
}

impl PlanStore {
    #[must_use]
    pub fn new(initial: Arc<ExecutionPlan>) -> Self {
        Self {
            active: ArcSwap::from(initial),
            writer: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn load(&self) -> Guard<Arc<ExecutionPlan>> {
        self.active.load()
    }

    /// Atomically installs a newer immutable execution plan.
    ///
    /// # Errors
    ///
    /// Returns [`PlanInstallError::StaleRevision`] when the proposed revision
    /// is not newer than the plan currently in use.
    pub fn install(&self, proposed: Arc<ExecutionPlan>) -> Result<(), PlanInstallError> {
        let _writer = self.writer.lock();
        let active_revision = self.active.load().revision();
        if proposed.revision() <= active_revision {
            return Err(PlanInstallError::StaleRevision {
                active: active_revision,
                proposed: proposed.revision(),
            });
        }
        self.active.store(proposed);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PlanInstallError {
    #[error("plan revision {proposed} is not newer than active revision {active}")]
    StaleRevision { active: u64, proposed: u64 },
}

#[derive(Debug, Default)]
pub struct RuntimeCounters {
    underruns: AtomicU64,
    overruns: AtomicU64,
}

impl RuntimeCounters {
    pub fn record_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_overrun(&self) {
        self.overruns.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> XrunCounters {
        XrunCounters {
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct XrunCounters {
    pub underruns: u64,
    pub overruns: u64,
}

pub struct AudioEngine {
    plans: PlanStore,
    counters: RuntimeCounters,
}

impl AudioEngine {
    #[must_use]
    pub fn new(initial_plan: Arc<ExecutionPlan>) -> Self {
        Self {
            plans: PlanStore::new(initial_plan),
            counters: RuntimeCounters::default(),
        }
    }

    #[must_use]
    pub fn active_plan(&self) -> Guard<Arc<ExecutionPlan>> {
        self.plans.load()
    }

    /// Atomically installs a newer execution plan without mutating the plan
    /// observed by an in-flight audio render block.
    ///
    /// # Errors
    ///
    /// Returns [`PlanInstallError`] when the proposed plan is stale.
    pub fn install_plan(&self, plan: Arc<ExecutionPlan>) -> Result<(), PlanInstallError> {
        self.plans.install(plan)
    }

    #[must_use]
    pub const fn counters(&self) -> &RuntimeCounters {
        &self.counters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonenari_audio_graph::{
        CompileContext, CompileLimits, GraphCompiler, GraphDocument, RenderFormat,
    };

    fn plan(revision: u64) -> Arc<ExecutionPlan> {
        Arc::new(
            GraphCompiler::default()
                .compile(
                    &GraphDocument::new(revision, vec![], vec![]),
                    CompileContext {
                        format: RenderFormat {
                            sample_rate: 192_000,
                            block_frames: 64,
                        },
                        limits: CompileLimits::default(),
                    },
                )
                .expect("empty graph should compile"),
        )
    }

    #[test]
    fn installs_only_newer_immutable_plans() {
        let engine = AudioEngine::new(plan(3));

        engine
            .install_plan(plan(4))
            .expect("newer revision should install");
        assert_eq!(engine.active_plan().revision(), 4);
        assert_eq!(engine.active_plan().format().sample_rate, 192_000);

        assert_eq!(
            engine.install_plan(plan(4)),
            Err(PlanInstallError::StaleRevision {
                active: 4,
                proposed: 4,
            })
        );
        assert_eq!(
            engine.install_plan(plan(2)),
            Err(PlanInstallError::StaleRevision {
                active: 4,
                proposed: 2,
            })
        );
    }

    #[test]
    fn snapshots_lock_free_runtime_counters() {
        let engine = AudioEngine::new(plan(1));
        assert_eq!(engine.counters().snapshot(), XrunCounters::default());

        engine.counters().record_underrun();
        engine.counters().record_overrun();
        engine.counters().record_overrun();

        assert_eq!(
            engine.counters().snapshot(),
            XrunCounters {
                underruns: 1,
                overruns: 2,
            }
        );
    }
}

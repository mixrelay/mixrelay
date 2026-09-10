//! Private Aggregate ownership, admission, readiness budget and retirement.
//! Native calls perform one operation; Rust retains every partially created owner.
use super::{
    admission::{Admission, Permit},
    ffi::{mr_aggregate_group_create, mr_aggregate_group_ready, mr_aggregate_group_try_destroy},
    group_plan::GroupPlan,
    retirement::{Retirable, Retirement},
};
use std::{ffi::c_void, ptr::NonNull, sync::Arc, thread, time::Duration};

static GROUPS: Admission = Admission::new(32);
static RETIRED: Retirement<GroupOwner> = Retirement::new();

struct GroupOwner {
    pointer: Option<NonNull<c_void>>,
    _permit: Permit<'static>,
}
// SAFETY: transferred only after the last IOProc owner retires; no callback or
// query can access a group in RETIRED. The control-side list serializes retries.
unsafe impl Send for GroupOwner {}
impl Retirable for GroupOwner {
    fn try_release(&mut self) -> bool {
        if let Some(pointer) = self.pointer {
            if !unsafe { mr_aggregate_group_try_destroy(pointer.as_ptr()) } {
                return false;
            }
            self.pointer = None;
        }
        true
    }
}

/// Every native registration retains this Arc until callback teardown succeeds.
pub(super) struct Aggregate {
    pub(super) pointer: NonNull<c_void>,
    pub(super) members: Vec<String>,
    owner: Option<GroupOwner>,
}
// SAFETY: immutable metadata and native property reads; Arc excludes retirement
// during a query. Destruction transfers exclusive ownership to the control list.
unsafe impl Send for Aggregate {}
unsafe impl Sync for Aggregate {}
impl Drop for Aggregate {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            RETIRED.retire(owner);
        }
    }
}
impl Aggregate {
    pub(super) fn active_owners() -> usize {
        GROUPS.in_use()
    }
    pub(super) fn ready(&self) -> bool {
        unsafe { mr_aggregate_group_ready(self.pointer.as_ptr()) }
    }
    pub(super) fn reclaim_retired() {
        RETIRED.reclaim();
    }

    pub(super) fn create(members: Vec<String>) -> Result<Option<Arc<Self>>, String> {
        let Some(plan) = GroupPlan::prepare(&members)? else {
            return Ok(None);
        };
        let permit = GROUPS
            .acquire()
            .ok_or("CoreAudio private group limit reached; native retirement pending")?;
        let pointers = plan
            .names
            .iter()
            .map(|uid| uid.as_ptr())
            .collect::<Vec<_>>();
        let mut status = 0;
        let pointer = unsafe {
            mr_aggregate_group_create(
                pointers.as_ptr(),
                plan.facts.as_ptr(),
                u32::try_from(pointers.len()).map_err(|error| error.to_string())?,
                0,
                &raw mut status,
            )
        };
        let Some(pointer) = NonNull::new(pointer) else {
            return if status == 1 {
                Ok(None)
            } else {
                Err("CoreAudio private group allocation failed".into())
            };
        };
        // Install the Rust guard BEFORE checking creation status. The C boundary
        // transfers partial objects too, because cleanup may itself fail.
        let group = Self {
            pointer,
            members,
            owner: Some(GroupOwner {
                pointer: Some(pointer),
                _permit: permit,
            }),
        };
        match status {
            0 => {
                if !confirm_ready(
                    || group.ready(),
                    || thread::sleep(Duration::from_millis(20)),
                ) {
                    return Err(
                        "CoreAudio private aggregate clock-policy/geometry readback timed out"
                            .into(),
                    );
                }
                Ok(Some(Arc::new(group)))
            }
            1 => Ok(None), // Incompatible rates/user aggregate: retain independent bridges.
            _ => Err("CoreAudio private aggregate native creation failed".into()),
        }
    }
}

/// A bounded readiness policy, not a retry of failed native creation. No `IOProc`
/// is registered until the already-created object's complete geometry is visible.
fn confirm_ready(mut ready: impl FnMut() -> bool, mut wait: impl FnMut()) -> bool {
    for attempt in 0..100 {
        if ready() {
            return true;
        }
        if attempt < 99 {
            wait();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::{device::Device, ffi::mr_device_io_native_frames, spec::Spec};
    use super::*;
    use mixrelay_audio_graph::DeviceSelector;
    #[test]
    fn readiness_is_bounded_and_never_accepts_an_unconfirmed_object() {
        let mut checks = 0;
        let mut waits = 0;
        assert!(!confirm_ready(
            || {
                checks += 1;
                false
            },
            || waits += 1
        ));
        assert_eq!((checks, waits), (100, 99));
        let mut checks = 0;
        let mut waits = 0;
        assert!(confirm_ready(
            || {
                checks += 1;
                checks == 3
            },
            || waits += 1
        ));
        assert_eq!((checks, waits), (3, 2));
    }
    #[test]
    fn partial_native_creation_returns_admission_after_rejected_members() {
        if !mixrelay_macos_test_support::memory_child(
            "macos_devices::aggregate::tests::partial_native_creation_returns_admission_after_rejected_members",
        ) {
            return;
        }
        // Exercise the low-level partial-owner contract directly: invalid UTF-8
        // fails after native allocation but before any OS device is created.
        // Production names are valid Rust strings, so use an explicit ABI fault.
        for _ in 0..128 {
            let names = [c"valid".as_ptr(), c"\xff".as_ptr()];
            let facts = [super::super::group_plan::MemberInfo::default(); 2];
            let permit = GROUPS.acquire().expect("partial owners must retire");
            let mut status = 0;
            let pointer = NonNull::new(unsafe {
                mr_aggregate_group_create(names.as_ptr(), facts.as_ptr(), 2, 0, &raw mut status)
            })
            .expect("partial allocation transferred");
            let owner = GroupOwner {
                pointer: Some(pointer),
                _permit: permit,
            };
            RETIRED.retire(owner);
            assert_eq!(status, 2);
        }
    }

    #[test]
    fn native_private_group_retains_owner_until_last_callback_retires() {
        if !mixrelay_macos_test_support::memory_child(
            "macos_devices::aggregate::tests::native_private_group_retains_owner_until_last_callback_retires",
        ) {
            return;
        }
        let primary = std::env::var("MIXRELAY_GROUP_PRIMARY_UID").expect("primary output UID");
        let secondary =
            std::env::var("MIXRELAY_GROUP_SECONDARY_UID").expect("secondary output UID");
        let baseline = GROUPS.in_use();
        assert_stale_plans_rejected(&primary, &secondary);
        assert_group_lifetime(primary, secondary, baseline);
    }

    fn assert_stale_plans_rejected(primary: &str, secondary: &str) {
        let baseline = GROUPS.in_use();
        // Native revalidation must reject a stale Rust plan before creating an
        // OS aggregate. Read real facts, then perturb only our local snapshot.
        let plan = GroupPlan::prepare(&[primary.to_owned(), secondary.to_owned()])
            .unwrap()
            .expect("compatible members");
        let names = plan
            .names
            .iter()
            .map(|uid| uid.as_ptr())
            .collect::<Vec<_>>();
        for changed in 0..3 {
            let mut facts = plan.facts.clone();
            match changed {
                0 => facts[0].device = 0,
                1 => facts[0].output += 1,
                _ => facts[0].rate += 1.0,
            }
            let permit = GROUPS.acquire().unwrap();
            let mut status = 0;
            let pointer = NonNull::new(unsafe {
                mr_aggregate_group_create(names.as_ptr(), facts.as_ptr(), 2, 0, &raw mut status)
            })
            .expect("stale plan transfers partial owner");
            RETIRED.retire(GroupOwner {
                pointer: Some(pointer),
                _permit: permit,
            });
            assert_eq!(status, 2, "stale identity/geometry/rate accepted");
            assert_eq!(GROUPS.in_use(), baseline);
        }
    }

    fn assert_group_lifetime(primary: String, secondary: String, baseline: usize) {
        let group = Aggregate::create(vec![primary.clone(), secondary.clone()])
            .unwrap()
            .expect("compatible members");
        let weak = Arc::downgrade(&group);
        let open = |uid| {
            Device::open_with_group(
                &Spec {
                    selector: DeviceSelector::PersistentUid { uid },
                    input: false,
                    channels: 2,
                },
                48_000,
                Some(group.clone()),
                0,
            )
            .unwrap()
        };
        let first = open(primary);
        let second = open(secondary);
        assert!(first.ready() && second.ready());
        assert!(unsafe { mr_device_io_native_frames(first.native.pointer.as_ptr()) } > 0);
        assert!(unsafe { mr_device_io_native_frames(second.native.pointer.as_ptr()) } > 0);
        let first_clock = first
            .clock_identity
            .load(std::sync::atomic::Ordering::Acquire);
        let second_clock = second
            .clock_identity
            .load(std::sync::atomic::Ordering::Acquire);
        assert_ne!(first_clock, 0);
        assert_eq!(first_clock, second_clock);
        drop(group);
        assert!(weak.upgrade().is_some());
        drop(first);
        assert!(
            weak.upgrade().is_some(),
            "second registration still owns group"
        );
        drop(second);
        assert!(weak.upgrade().is_none());
        Aggregate::reclaim_retired();
        assert_eq!(GROUPS.in_use(), baseline);
        eprintln!(
            "private group shared clock={first_clock}; 2 native owners retired; group admission restored to {baseline}"
        );
    }
}

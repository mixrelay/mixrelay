use super::*;

#[test]
fn delayed_first_use_consent_is_distinct_from_device_failure() {
    let failure = || Err(RecvTimeoutError::Timeout);
    assert_eq!(
        initialization(failure(), Some(InputPermission::Pending), || {
            InputPermission::Pending
        }),
        Err(HostAudioBackendError::PermissionPending {
            capability: "microphone".into()
        })
    );
    assert_eq!(
        initialization(failure(), Some(InputPermission::Denied), || {
            InputPermission::Denied
        }),
        Err(HostAudioBackendError::PermissionDenied {
            capability: "microphone".into()
        })
    );
    for permission in [
        None,
        Some(InputPermission::Authorized),
        Some(InputPermission::Unknown),
    ] {
        assert!(matches!(
            initialization(failure(), permission, || permission.unwrap()),
            Err(HostAudioBackendError::BackendFailure { .. })
        ));
    }
}

#[test]
fn first_prompt_decision_is_observed_and_late_approval_does_not_become_a_disconnect() {
    assert_eq!(
        initialization(
            Ok(Err("input access failed".into())),
            Some(InputPermission::Pending),
            || InputPermission::Denied
        ),
        Err(HostAudioBackendError::PermissionDenied {
            capability: "microphone".into()
        })
    );
    assert_eq!(
        initialization(
            Err(RecvTimeoutError::Timeout),
            Some(InputPermission::Pending),
            || InputPermission::Authorized
        ),
        Err(HostAudioBackendError::PermissionPending {
            capability: "microphone".into()
        })
    );
    assert!(matches!(
        initialization(Err(RecvTimeoutError::Timeout), None, || panic!(
            "output-only initialization must not query microphone consent"
        )),
        Err(HostAudioBackendError::BackendFailure { .. })
    ));
}

#[test]
fn consent_does_not_override_native_readiness_or_a_failed_worker() {
    assert_eq!(
        initialization(Ok(Ok(())), Some(InputPermission::Pending), || panic!(
            "ready inputs do not need another query"
        )),
        Ok(())
    );
    assert_eq!(
        initialization(Ok(Ok(())), Some(InputPermission::Denied), || panic!(
            "ready inputs do not need another query"
        )),
        Ok(())
    );
    for failure in [
        Err(RecvTimeoutError::Disconnected),
        Ok(Err("native device disappeared".into())),
    ] {
        assert!(matches!(
            initialization(failure, Some(InputPermission::Pending), || {
                InputPermission::Pending
            }),
            Err(HostAudioBackendError::BackendFailure { .. })
        ));
    }
}

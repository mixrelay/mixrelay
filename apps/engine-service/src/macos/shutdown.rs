//! Complete graph/recording retirement before stopping independent Driver audio.
use std::{future::Future, path::Path, time::Duration};

pub(super) async fn wait_for_controller(path: impl AsRef<Path>) {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
        interval.tick().await;
        match mixrelay_macos_transport::app_lifecycle::is_owned(&path) {
            Ok(true) => continue,
            Ok(false) => tracing::info!("controller released ownership; finalizing audio server"),
            Err(error) => {
                tracing::error!(%error, "controller lease probe failed; stopping audio server");
            }
        }
        return;
    }
}

pub(super) async fn finish(
    retired: Result<(), String>,
    stop_driver: impl Future<Output = Result<(), String>>,
) -> Result<(), String> {
    // Attempt the native stop even when retirement reports a failure. Reporting
    // that error must not leave a separate HAL loopback running indefinitely.
    match (retired, stop_driver.await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(format!("Driver shutdown failed: {error}")),
        (Err(retirement), Err(driver)) => {
            Err(format!("{retirement}\nDriver shutdown failed: {driver}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    #[ignore = "subprocess fixture; launched only by the ownership-loss test"]
    fn lease_owner_fixture() {
        let Some(path) = std::env::var_os("MIXRELAY_BACKGROUND_STOP_TEST_LEASE") else {
            return;
        };
        let lease = mixrelay_macos_transport::app_lifecycle::ProcessLease::acquire(path)
            .expect("isolated test lease");
        lease.record("ready").unwrap();
        loop {
            std::thread::park();
        }
    }

    #[tokio::test]
    async fn killed_controller_releases_os_lease_and_runs_driver_stop() {
        let directory = mixrelay_test_support::FixtureDir::new("background-stop").unwrap();
        let path = directory.join("controller.owner");
        std::fs::File::create(&path).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "macos::shutdown::tests::lease_owner_fixture",
            ])
            .env("MIXRELAY_BACKGROUND_STOP_TEST_LEASE", &path)
            .spawn()
            .unwrap();
        let mut ready = false;
        for _ in 0..200 {
            if std::fs::read_to_string(&path).unwrap() == "ready" {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Always reap the fixture before an assertion can unwind the test.
        if !ready {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lease fixture did not become ready");
        }
        let live =
            tokio::time::timeout(Duration::from_millis(150), wait_for_controller(&path)).await;
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        assert!(
            live.is_err(),
            "a live background controller must retain audio"
        );
        tokio::time::timeout(Duration::from_secs(1), wait_for_controller(&path))
            .await
            .expect("SIGKILL releases the process-owned lock");
        let stopped = AtomicBool::new(false);
        finish(Ok(()), async {
            stopped.store(true, Ordering::Release);
            Ok(())
        })
        .await
        .unwrap();
        assert!(stopped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn retirement_failure_still_stops_native_audio() {
        let stopped = AtomicBool::new(false);
        let result = finish(Err("recording finalization failed".into()), async {
            stopped.store(true, Ordering::Release);
            Ok(())
        })
        .await;
        assert!(stopped.load(Ordering::Acquire));
        assert_eq!(result, Err("recording finalization failed".into()));
    }

    #[tokio::test]
    async fn native_stop_failure_is_not_a_successful_engine_exit() {
        assert_eq!(
            finish(Ok(()), async { Err("devices did not stop".into()) }).await,
            Err("Driver shutdown failed: devices did not stop".into())
        );
        let errors = finish(Err("recording failed".into()), async {
            Err("state was not saved".into())
        })
        .await
        .unwrap_err();
        assert!(errors.contains("recording failed"));
        assert!(errors.contains("state was not saved"));
    }
}

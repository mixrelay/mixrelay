//! Preserve only the apps this authorized transaction actually closes.

use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SessionApp {
    Graph,
    /// The preceding app-owned controller, closed during migration.
    LegacyService,
    Service,
    Driver,
    /// The background role of the unified `MixRelay` bundle.
    UnifiedRuntime,
}

impl SessionApp {
    pub(super) const ALL: [Self; 5] = [
        Self::Graph,
        Self::LegacyService,
        Self::Service,
        Self::Driver,
        Self::UnifiedRuntime,
    ];

    pub(super) const fn bundle(self) -> &'static str {
        match self {
            Self::Graph | Self::LegacyService | Self::UnifiedRuntime => {
                "/Applications/MixRelay.app"
            }
            Self::Driver | Self::Service => "/Applications/MixRelay Driver.app",
        }
    }

    pub(super) const fn identifier(self) -> &'static str {
        match self {
            Self::Graph | Self::LegacyService | Self::UnifiedRuntime => "io.mixrelay.desktop",
            Self::Driver | Self::Service => "io.mixrelay.driver.desktop",
        }
    }

    pub(super) fn matches_command(self, command: &str) -> bool {
        let executable = format!("{}/Contents/MacOS/mixrelay-desktop", self.bundle());
        match self {
            Self::LegacyService => command == format!("{executable} --background-service"),
            Self::Service | Self::UnifiedRuntime => {
                command == format!("{executable} --driver-runtime")
            }
            Self::Graph | Self::Driver => command == executable,
        }
    }

    pub(super) const fn quit_action(self) -> &'static str {
        match self {
            Self::Graph => "quit-graph-app",
            Self::LegacyService => "quit-background-service",
            Self::Service => "quit-driver-runtime",
            Self::Driver => "quit-driver-manager",
            Self::UnifiedRuntime => "quit-mixrelay-runtime",
        }
    }

    pub(super) const fn restore_action(self) -> &'static str {
        match self {
            Self::Graph => "restore-graph-app",
            Self::LegacyService => "restore-background-service",
            Self::Service => "restore-driver-runtime",
            Self::Driver => "restore-driver-manager",
            Self::UnifiedRuntime => "restore-mixrelay-runtime",
        }
    }
}

pub(super) fn run_with_apps_closed<T>(
    running: &[SessionApp],
    install: bool,
    mut action: impl FnMut(&'static str) -> Result<String>,
    transaction: impl FnOnce() -> Result<T>,
    can_recover: impl FnOnce() -> bool,
) -> Result<T> {
    let mut closed = Vec::new();
    let result = (|| {
        for app in running {
            // A refused/timed-out Quit can have already closed some processes.
            closed.push(*app);
            let acknowledgement = action(app.quit_action())?;
            ensure!(
                matches!(acknowledgement.trim(), "true" | "false"),
                "invalid app Quit acknowledgement"
            );
            if acknowledgement.trim() == "false" {
                closed.pop();
            }
        }
        transaction()
    })();
    if (result.is_ok() && install) || (result.is_err() && can_recover()) {
        // Restore Driver before the graph app; attempt both even if one fails.
        let mut restoration_error = None;
        for app in closed.iter().rev() {
            if let Err(error) = action(app.restore_action()) {
                restoration_error.get_or_insert(error);
            }
        }
        if let Some(error) = restoration_error {
            return match result {
                Ok(_) => Err(error.context("update completed but app relaunch failed")),
                Err(original) => {
                    Err(original.context(format!("app restoration failed: {error:#}")))
                }
            };
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn update_reopens_editor_controller_and_driver_only_after_commit() {
        let events = RefCell::new(Vec::new());
        run_with_apps_closed(
            &SessionApp::ALL,
            true,
            |action| {
                events.borrow_mut().push(action);
                Ok("true".into())
            },
            || {
                events.borrow_mut().push("commit");
                Ok(())
            },
            || panic!("successful update requires no recovery"),
        )
        .unwrap();
        assert_eq!(
            *events.borrow(),
            [
                "quit-graph-app",
                "quit-background-service",
                "quit-driver-runtime",
                "quit-driver-manager",
                "quit-mixrelay-runtime",
                "commit",
                "restore-mixrelay-runtime",
                "restore-driver-manager",
                "restore-driver-runtime",
                "restore-background-service",
                "restore-graph-app"
            ]
        );
    }

    #[test]
    fn first_install_and_stopped_update_launch_nothing() {
        run_with_apps_closed(
            &[],
            true,
            |_| panic!("no running apps"),
            || Ok(()),
            || false,
        )
        .unwrap();
    }

    #[test]
    fn app_that_exited_before_quit_is_not_reopened() {
        let mut actions = Vec::new();
        run_with_apps_closed(
            &SessionApp::ALL,
            true,
            |action| {
                actions.push(action);
                Ok(if action == "quit-graph-app" {
                    "false"
                } else {
                    "true"
                }
                .into())
            },
            || Ok(()),
            || false,
        )
        .unwrap();
        assert_eq!(
            actions,
            [
                "quit-graph-app",
                "quit-background-service",
                "quit-driver-runtime",
                "quit-driver-manager",
                "quit-mixrelay-runtime",
                "restore-mixrelay-runtime",
                "restore-driver-manager",
                "restore-driver-runtime",
                "restore-background-service"
            ]
        );
    }

    #[test]
    fn successful_removal_never_reopens_apps() {
        let mut actions = Vec::new();
        run_with_apps_closed(
            &[SessionApp::Driver],
            false,
            |action| {
                actions.push(action);
                Ok("true".into())
            },
            || Ok(()),
            || panic!("no recovery"),
        )
        .unwrap();
        assert_eq!(actions, ["quit-driver-manager"]);
    }

    #[test]
    fn failed_update_reopens_only_when_installed_state_can_be_recovered() {
        for recover in [false, true] {
            let mut actions = Vec::new();
            let result: Result<()> = run_with_apps_closed(
                &[SessionApp::Driver],
                true,
                |action| {
                    actions.push(action);
                    Ok("true".into())
                },
                || anyhow::bail!("commit failed"),
                || recover,
            );
            assert!(result.is_err());
            assert_eq!(actions.len(), if recover { 2 } else { 1 });
        }
    }

    #[test]
    fn quit_failure_does_not_execute_transaction_and_restores_attempted_apps() {
        let mut actions = Vec::new();
        let result: Result<()> = run_with_apps_closed(
            &SessionApp::ALL,
            true,
            |action| {
                actions.push(action);
                if action == "quit-driver-manager" {
                    anyhow::bail!("Quit timed out");
                }
                Ok("true".into())
            },
            || panic!("Quit did not complete"),
            || true,
        );
        assert!(result.is_err());
        assert_eq!(
            actions,
            [
                "quit-graph-app",
                "quit-background-service",
                "quit-driver-runtime",
                "quit-driver-manager",
                "restore-driver-manager",
                "restore-driver-runtime",
                "restore-background-service",
                "restore-graph-app"
            ]
        );
    }

    #[test]
    fn relaunch_failure_does_not_skip_other_app_or_repeat_commit() {
        let events = RefCell::new(Vec::new());
        let result: Result<()> = run_with_apps_closed(
            &SessionApp::ALL,
            true,
            |action| {
                events.borrow_mut().push(action);
                if action == "restore-driver-manager" {
                    anyhow::bail!("launch failed");
                }
                Ok("true".into())
            },
            || {
                events.borrow_mut().push("commit");
                Ok(())
            },
            || false,
        );
        assert!(result.unwrap_err().to_string().contains("update completed"));
        assert_eq!(
            *events.borrow(),
            [
                "quit-graph-app",
                "quit-background-service",
                "quit-driver-runtime",
                "quit-driver-manager",
                "quit-mixrelay-runtime",
                "commit",
                "restore-mixrelay-runtime",
                "restore-driver-manager",
                "restore-driver-runtime",
                "restore-background-service",
                "restore-graph-app"
            ]
        );
    }
}

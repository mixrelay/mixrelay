//! Retries recovery only while the controller still owns the audio lifetime.

use std::{future::Future, time::Duration};

/// Observation failures never authorize replacing a potentially live process.
///
/// `recover` returns a future that owns what it needs: a future borrowing the caller's
/// state through an async closure cannot be proven `Send` for a spawned task.
pub(crate) async fn supervise<E: std::fmt::Display, R: Future<Output = Result<(), E>>>(
    stopping: impl Fn() -> bool,
    mut exited: impl FnMut() -> Result<bool, E>,
    mut recover: impl FnMut() -> R,
) {
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if stopping() {
            return;
        }
        match exited() {
            Ok(false) => {}
            Ok(true) => {
                if stopping() {
                    return;
                }
                if let Err(error) = recover().await {
                    tracing::error!(%error, "owned audio engine recovery failed; retrying");
                }
            }
            Err(error) => tracing::error!(%error, "could not observe owned audio engine"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test(start_paused = true)]
    async fn abnormal_exit_retries_failed_replacement_and_never_restarts_after_stop() {
        let stopped = Cell::new(false);
        let polls = Cell::new(0);
        let replacements = Cell::new(0);
        let running = Cell::new(false);
        tokio::time::timeout(
            Duration::from_secs(5),
            supervise(
                || stopped.get(),
                || {
                    polls.set(polls.get() + 1);
                    if polls.get() == 1 {
                        return Err("observation failed");
                    }
                    if running.get() {
                        // Simulate another abnormal exit racing an explicit stop.
                        running.set(false);
                        stopped.set(true);
                        return Ok(true);
                    }
                    Ok(true)
                },
                || {
                    replacements.set(replacements.get() + 1);
                    let replaced = if replacements.get() == 1 {
                        Err("spawn failed")
                    } else {
                        running.set(true);
                        Ok(())
                    };
                    std::future::ready(replaced)
                },
            ),
        )
        .await
        .unwrap();
        assert_eq!(polls.get(), 4);
        assert_eq!(replacements.get(), 2);
        assert!(!running.get());
    }
}

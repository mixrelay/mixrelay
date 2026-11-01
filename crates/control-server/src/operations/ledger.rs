//! Operation receipts: admission, deduplication, completion, acknowledgement and expiry.
//!
//! Callers pass the current time and new identities in, so every transition can be
//! tested without a runtime or real time. `R` is the stored response and `W` whatever
//! the caller wakes when an operation completes.
use std::{collections::HashMap, time::Duration};

use mixrelay_time::Instant;

pub(super) struct ReceiptLedger<R, W> {
    capacity: usize,
    completed_ttl: Duration,
    admission_open: bool,
    pending: usize,
    receipts: HashMap<String, Receipt<R, W>>,
}

struct Receipt<R, W> {
    waiter: W,
    outcome: Outcome<R>,
}

enum Outcome<R> {
    Pending,
    Complete { response: R, completed_at: Instant },
}

/// What a receipt says about an operation.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Snapshot<R> {
    Pending,
    Complete(R),
    Missing,
}

/// Why `admit` did not start a new operation.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Rejection<R> {
    /// The client ID already names a running operation.
    Pending,
    /// The client ID already names a completed operation; replay its response.
    Complete(R),
    /// Admission is closed or every receipt is in use.
    Full,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ack {
    Completed,
    Pending,
    Missing,
}

impl<R: Clone, W: Clone> ReceiptLedger<R, W> {
    pub(super) fn new(capacity: usize, completed_ttl: Duration) -> Self {
        Self {
            capacity,
            completed_ttl,
            admission_open: true,
            pending: 0,
            receipts: HashMap::new(),
        }
    }

    /// Operations admitted and not yet complete.
    pub(super) const fn pending(&self) -> usize {
        self.pending
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Stops new admissions. Known client IDs still resolve to their receipts.
    pub(super) fn close(&mut self) {
        self.admission_open = false;
    }

    /// Starts an operation under the client's ID, or under the first identity from
    /// `issue` that no receipt uses. A known client ID never starts work again.
    pub(super) fn admit(
        &mut self,
        requested: Option<&str>,
        waiter: W,
        now: Instant,
        mut issue: impl FnMut() -> String,
    ) -> Result<String, Rejection<R>> {
        self.purge_expired(now);
        if let Some(id) = requested
            && let Some(receipt) = self.receipts.get(id)
        {
            return Err(match &receipt.outcome {
                Outcome::Pending => Rejection::Pending,
                Outcome::Complete { response, .. } => Rejection::Complete(response.clone()),
            });
        }
        if !self.admission_open || self.receipts.len() >= self.capacity {
            return Err(Rejection::Full);
        }
        let id = requested.map_or_else(
            || loop {
                let id = issue();
                if !self.receipts.contains_key(&id) {
                    break id;
                }
            },
            str::to_owned,
        );
        self.receipts.insert(
            id.clone(),
            Receipt {
                waiter,
                outcome: Outcome::Pending,
            },
        );
        self.pending += 1;
        Ok(id)
    }

    /// Records the response of a pending operation and returns its waiter to wake.
    /// A missing or already completed receipt is left unchanged.
    pub(super) fn complete(&mut self, id: &str, response: R, now: Instant) -> Option<W> {
        let receipt = self.receipts.get_mut(id)?;
        if !matches!(receipt.outcome, Outcome::Pending) {
            return None;
        }
        receipt.outcome = Outcome::Complete {
            response,
            completed_at: now,
        };
        self.pending = self.pending.saturating_sub(1);
        Some(receipt.waiter.clone())
    }

    pub(super) fn snapshot(&mut self, id: &str, now: Instant) -> Snapshot<R> {
        self.purge_expired(now);
        match self.receipts.get(id).map(|receipt| &receipt.outcome) {
            Some(Outcome::Pending) => Snapshot::Pending,
            Some(Outcome::Complete { response, .. }) => Snapshot::Complete(response.clone()),
            None => Snapshot::Missing,
        }
    }

    /// Removes a completed receipt so its ID and slot can be reused.
    pub(super) fn acknowledge(&mut self, id: &str, now: Instant) -> Ack {
        self.purge_expired(now);
        match self.receipts.get(id).map(|receipt| &receipt.outcome) {
            None => Ack::Missing,
            Some(Outcome::Pending) => Ack::Pending,
            Some(Outcome::Complete { .. }) => {
                self.receipts.remove(id);
                Ack::Completed
            }
        }
    }

    /// Removes and returns a completed response that no client will poll for.
    pub(super) fn take_completed(&mut self, id: &str) -> Option<R> {
        let receipt = self.receipts.remove(id)?;
        if let Outcome::Complete { response, .. } = receipt.outcome {
            return Some(response);
        }
        self.receipts.insert(id.to_owned(), receipt);
        None
    }

    /// Completed receipts expire `completed_ttl` after completion; pending ones never do.
    fn purge_expired(&mut self, now: Instant) {
        let ttl = self.completed_ttl;
        self.receipts.retain(|_, receipt| {
            !matches!(&receipt.outcome, Outcome::Complete { completed_at, .. }
                if now.duration_since(*completed_at) >= ttl)
        });
    }
}

#[cfg(test)]
mod tests;

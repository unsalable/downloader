//! Pause / resume / cancel signalling for an in-flight download.
//!
//! A running transfer checks this between chunks. Pausing parks the task on a
//! `Notify` instead of spinning, so a paused download costs nothing; cancelling
//! wakes it so it can unwind immediately rather than after the next chunk.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

use crate::error::AppError;

const RUNNING: u8 = 0;
const PAUSED: u8 = 1;
const CANCELED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlState {
    Running,
    Paused,
    Canceled,
}

#[derive(Debug)]
pub struct TaskControl {
    state: AtomicU8,
    wake: Notify,
}

impl Default for TaskControl {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskControl {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(RUNNING),
            wake: Notify::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn state(&self) -> ControlState {
        match self.state.load(Ordering::Acquire) {
            PAUSED => ControlState::Paused,
            CANCELED => ControlState::Canceled,
            _ => ControlState::Running,
        }
    }

    pub fn pause(&self) {
        // Cancelling is final: a late pause must not revive a cancelled task.
        let _ = self.state.compare_exchange(
            RUNNING,
            PAUSED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub fn resume(&self) {
        if self
            .state
            .compare_exchange(PAUSED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.wake.notify_waiters();
        }
    }

    pub fn cancel(&self) {
        self.state.store(CANCELED, Ordering::Release);
        self.wake.notify_waiters();
    }

    pub fn is_canceled(&self) -> bool {
        self.state() == ControlState::Canceled
    }

    /// True when a transfer loop should stop, for either reason.
    ///
    /// Download loops use this rather than awaiting [`Self::checkpoint`]:
    /// parking mid-transfer would hold a connection open until the server timed
    /// it out. Unwinding and re-requesting with a `Range` header on resume is
    /// both cheaper and more reliable. The orchestrator inspects [`Self::state`]
    /// afterwards to tell a pause from a cancel.
    pub fn interrupted(&self) -> bool {
        self.state() != ControlState::Running
    }

    /// Block while paused. Returns `Err(Canceled)` if the task is cancelled,
    /// either on entry or while it waits.
    pub async fn checkpoint(&self) -> Result<(), AppError> {
        loop {
            match self.state() {
                ControlState::Running => return Ok(()),
                ControlState::Canceled => return Err(AppError::Canceled),
                ControlState::Paused => {
                    // Register interest before re-reading the state, so a
                    // resume between the check and the wait is not missed.
                    let notified = self.wake.notified();
                    if self.state() != ControlState::Paused {
                        continue;
                    }
                    notified.await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_running() {
        assert_eq!(TaskControl::new().state(), ControlState::Running);
    }

    #[test]
    fn pause_and_resume_round_trip() {
        let control = TaskControl::new();
        control.pause();
        assert_eq!(control.state(), ControlState::Paused);
        control.resume();
        assert_eq!(control.state(), ControlState::Running);
    }

    #[test]
    fn cancel_wins_over_a_later_pause() {
        let control = TaskControl::new();
        control.cancel();
        control.pause();
        assert_eq!(control.state(), ControlState::Canceled);
        control.resume();
        assert_eq!(control.state(), ControlState::Canceled);
    }

    #[tokio::test]
    async fn checkpoint_returns_immediately_while_running() {
        let control = TaskControl::new();
        assert!(control.checkpoint().await.is_ok());
    }

    #[tokio::test]
    async fn checkpoint_fails_once_cancelled() {
        let control = TaskControl::new();
        control.cancel();
        assert!(matches!(control.checkpoint().await, Err(AppError::Canceled)));
    }

    #[tokio::test]
    async fn a_paused_checkpoint_wakes_on_resume() {
        let control = TaskControl::shared();
        control.pause();

        let waiter = {
            let control = Arc::clone(&control);
            tokio::spawn(async move { control.checkpoint().await })
        };

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        control.resume();
        assert!(waiter.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn a_paused_checkpoint_wakes_on_cancel() {
        let control = TaskControl::shared();
        control.pause();

        let waiter = {
            let control = Arc::clone(&control);
            tokio::spawn(async move { control.checkpoint().await })
        };

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        control.cancel();
        assert!(matches!(waiter.await.unwrap(), Err(AppError::Canceled)));
    }
}

//! The download queue.
//!
//! Owns task state, decides what runs when, and is the only place that emits
//! progress to the UI. A single scheduler task does the admitting; worker tasks
//! do the downloading. The task list is behind a plain mutex that is never held
//! across an await, so a slow download cannot block a UI query.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::db::Database;
use crate::downloader::control::{ControlState, TaskControl};
use crate::error::{AppError, AppResult};
use crate::model::{
    DownloadProgress, DownloadRequest, DownloadStage, DownloadStatus, DownloadTask, HistoryEntry,
    ProgressEvent,
};
use crate::settings::Settings;
use crate::{downloader, log_error, log_info, util};

pub const EVENT_PROGRESS: &str = "download://progress";
pub const EVENT_CHANGED: &str = "download://changed";

pub struct QueueManager {
    app: AppHandle,
    db: Arc<Database>,
    settings: Arc<Mutex<Settings>>,
    tasks: Mutex<Vec<DownloadTask>>,
    controls: Mutex<HashMap<String, Arc<TaskControl>>>,
    running: AtomicU32,
    wake: Notify,
}

impl QueueManager {
    pub fn new(app: AppHandle, db: Arc<Database>, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            app,
            db,
            settings,
            tasks: Mutex::new(Vec::new()),
            controls: Mutex::new(HashMap::new()),
            running: AtomicU32::new(0),
            wake: Notify::new(),
        })
    }

    fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn with_tasks<T>(&self, f: impl FnOnce(&mut Vec<DownloadTask>) -> T) -> T {
        let mut guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub fn list(&self) -> Vec<DownloadTask> {
        self.with_tasks(|tasks| tasks.clone())
    }

    pub fn active_count(&self) -> u32 {
        self.with_tasks(|tasks| {
            tasks.iter().filter(|task| task.status.is_active()).count() as u32
        })
    }

    pub fn queued_count(&self) -> u32 {
        self.with_tasks(|tasks| {
            tasks
                .iter()
                .filter(|task| task.status == DownloadStatus::Queued)
                .count() as u32
        })
    }

    // -- lifecycle ---------------------------------------------------------

    /// Restore whatever was in flight when the app last closed. Anything that
    /// was mid-download comes back paused rather than silently resuming: the
    /// user did not ask for network activity at startup.
    pub fn restore(self: &Arc<Self>) {
        let Ok(mut restored) = self.db.queue_load() else {
            return;
        };

        for task in &mut restored {
            if task.status.is_active() || task.status == DownloadStatus::Queued {
                task.status = DownloadStatus::Paused;
                task.progress.speed_bps = 0.0;
                task.progress.eta_sec = None;
            }
        }

        if restored.is_empty() {
            return;
        }
        log_info!("queue", "restored {} tasks", restored.len());
        self.with_tasks(|tasks| *tasks = restored);
        self.emit_changed();
    }

    pub fn enqueue(self: &Arc<Self>, request: DownloadRequest) -> DownloadTask {
        let now = util::now_ms();
        let task = DownloadTask {
            id: util::new_id("dl"),
            url: request.url.clone(),
            title: request
                .title
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| request.url.clone()),
            platform: downloader::platform_of(&request),
            thumbnail_url: request.thumbnail_url.clone(),
            status: DownloadStatus::Queued,
            progress: DownloadProgress::default(),
            format_label: downloader::provisional_label(&request),
            output_path: None,
            error: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            attempt: 0,
            request,
        };

        self.with_tasks(|tasks| tasks.push(task.clone()));
        self.persist();
        self.emit_changed();
        self.wake.notify_one();
        task
    }

    pub fn pause(self: &Arc<Self>, id: &str) {
        if let Some(control) = self.control_for(id) {
            control.pause();
        }
        // A task that has not started yet has no control handle, so its status
        // is set directly.
        self.update_task(id, |task| {
            if task.status == DownloadStatus::Queued {
                task.status = DownloadStatus::Paused;
            }
        });
        self.emit_changed();
    }

    pub fn resume(self: &Arc<Self>, id: &str) {
        self.update_task(id, |task| {
            if matches!(task.status, DownloadStatus::Paused | DownloadStatus::Failed) {
                task.status = DownloadStatus::Queued;
                task.error = None;
            }
        });
        self.persist();
        self.emit_changed();
        self.wake.notify_one();
    }

    pub fn cancel(self: &Arc<Self>, id: &str) {
        if let Some(control) = self.control_for(id) {
            control.cancel();
        }
        self.update_task(id, |task| {
            if !task.status.is_terminal() {
                task.status = DownloadStatus::Canceled;
                task.progress.speed_bps = 0.0;
                task.progress.eta_sec = None;
            }
        });
        downloader::cleanup_task_files(id);
        self.persist();
        self.emit_changed();
        self.wake.notify_one();
    }

    pub fn retry(self: &Arc<Self>, id: &str) {
        self.update_task(id, |task| {
            if task.status.is_terminal() {
                task.status = DownloadStatus::Queued;
                task.error = None;
                task.attempt = 0;
                task.progress = DownloadProgress::default();
            }
        });
        self.persist();
        self.emit_changed();
        self.wake.notify_one();
    }

    pub fn remove(self: &Arc<Self>, id: &str) {
        if let Some(control) = self.control_for(id) {
            control.cancel();
        }
        self.with_tasks(|tasks| tasks.retain(|task| task.id != id));
        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        downloader::cleanup_task_files(id);
        self.persist();
        self.emit_changed();
    }

    pub fn pause_all(self: &Arc<Self>) {
        let ids = self.with_tasks(|tasks| {
            tasks
                .iter_mut()
                .filter(|task| !task.status.is_terminal())
                .map(|task| {
                    if task.status == DownloadStatus::Queued {
                        task.status = DownloadStatus::Paused;
                    }
                    task.id.clone()
                })
                .collect::<Vec<_>>()
        });

        let controls = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            if let Some(control) = controls.get(&id) {
                control.pause();
            }
        }
        drop(controls);
        self.emit_changed();
    }

    pub fn resume_all(self: &Arc<Self>) {
        self.with_tasks(|tasks| {
            for task in tasks.iter_mut() {
                if task.status == DownloadStatus::Paused {
                    task.status = DownloadStatus::Queued;
                    task.error = None;
                }
            }
        });
        self.persist();
        self.emit_changed();
        self.wake.notify_waiters();
        self.wake.notify_one();
    }

    pub fn clear_finished(self: &Arc<Self>) {
        let removed: Vec<String> = self.with_tasks(|tasks| {
            let mut removed = Vec::new();
            tasks.retain(|task| {
                if task.status.is_terminal() {
                    removed.push(task.id.clone());
                    false
                } else {
                    true
                }
            });
            removed
        });

        for id in &removed {
            downloader::cleanup_task_files(id);
        }
        self.persist();
        self.emit_changed();
    }

    /// Move a waiting task one place up or down among the other waiting tasks.
    ///
    /// The swap deliberately skips over running and finished tasks. The UI
    /// groups the queue by status, so swapping with a raw neighbour that sits in
    /// a different group would reorder the underlying list while appearing to do
    /// nothing at all.
    pub fn reorder(self: &Arc<Self>, id: &str, delta: i32) {
        let moved = self.with_tasks(|tasks| {
            let Some(index) = tasks.iter().position(|task| task.id == id) else {
                return false;
            };
            if !is_waiting(&tasks[index]) {
                return false;
            }

            let step: i32 = if delta < 0 { -1 } else { 1 };
            let mut cursor = index as i32 + step;
            while cursor >= 0 && (cursor as usize) < tasks.len() {
                if is_waiting(&tasks[cursor as usize]) {
                    tasks.swap(index, cursor as usize);
                    return true;
                }
                cursor += step;
            }
            false
        });

        if moved {
            self.persist();
            self.emit_changed();
        }
    }

    /// Apply an explicit order, used by drag and drop.
    pub fn set_order(self: &Arc<Self>, ids: &[String]) {
        self.with_tasks(|tasks| {
            let mut ordered: Vec<DownloadTask> = Vec::with_capacity(tasks.len());
            for id in ids {
                if let Some(index) = tasks.iter().position(|task| &task.id == id) {
                    ordered.push(tasks.remove(index));
                }
            }
            // Anything the caller did not mention keeps its relative position
            // at the end, so a stale id list cannot drop tasks.
            ordered.append(tasks);
            *tasks = ordered;
        });
        self.persist();
        self.emit_changed();
    }

    // -- scheduling --------------------------------------------------------

    /// Long-lived admission loop. Started once at app setup.
    pub fn spawn_scheduler(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                let limit = manager.settings().max_concurrent_downloads;
                let running = manager.running.load(Ordering::Acquire);

                if running < limit {
                    if let Some(task) = manager.take_next_queued() {
                        manager.running.fetch_add(1, Ordering::AcqRel);
                        manager.spawn_worker(task);
                        // Loop again immediately: there may be another free slot.
                        continue;
                    }
                }

                manager.wake.notified().await;
            }
        });
    }

    fn take_next_queued(self: &Arc<Self>) -> Option<DownloadTask> {
        self.with_tasks(|tasks| {
            let task = tasks
                .iter_mut()
                .find(|task| task.status == DownloadStatus::Queued)?;
            task.status = DownloadStatus::Preparing;
            task.started_at = Some(util::now_ms());
            task.progress.stage = DownloadStage::Resolving;
            Some(task.clone())
        })
    }

    fn spawn_worker(self: &Arc<Self>, task: DownloadTask) {
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            manager.run_task(task).await;
            manager.running.fetch_sub(1, Ordering::AcqRel);
            manager.wake.notify_one();
        });
    }

    async fn run_task(self: &Arc<Self>, task: DownloadTask) {
        let control = TaskControl::shared();
        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(task.id.clone(), Arc::clone(&control));

        self.emit_changed();

        let settings = self.settings();
        let max_attempts = settings.auto_retry_count + 1;
        let mut attempt = 0u32;

        let outcome = loop {
            attempt += 1;
            self.update_task(&task.id, |current| {
                current.attempt = attempt;
                current.status = DownloadStatus::Downloading;
                current.error = None;
            });

            let mut sink = {
                let manager = Arc::clone(self);
                let id = task.id.clone();
                move |progress: DownloadProgress| {
                    manager.on_progress(&id, progress);
                }
            };

            let result = downloader::execute(
                &task.id,
                &task.request,
                &settings,
                Arc::clone(&control),
                &mut sink,
            )
            .await;

            match result {
                Ok(outcome) => break Ok(outcome),
                Err(AppError::Canceled) => break Err(AppError::Canceled),
                Err(err) => {
                    // Only retry things that can plausibly succeed next time,
                    // and never fight a user who is trying to stop the task.
                    if attempt >= max_attempts || !err.retryable() || control.interrupted() {
                        break Err(err);
                    }
                    log_info!(
                        "queue",
                        "task {} attempt {attempt} failed ({}), retrying",
                        task.id,
                        err.code()
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(
                        800 * u64::from(attempt.min(4)),
                    ))
                    .await;
                }
            }
        };

        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&task.id);

        match outcome {
            Ok(result) => self.complete(&task.id, result),
            Err(AppError::Canceled) => self.settle_interruption(&task.id, control.state()),
            Err(err) => self.fail(&task.id, err),
        }
    }

    fn complete(self: &Arc<Self>, id: &str, outcome: downloader::DownloadOutcome) {
        let path = outcome.output_path.to_string_lossy().into_owned();
        let now = util::now_ms();

        let task = self.update_task(id, |task| {
            task.status = DownloadStatus::Completed;
            task.completed_at = Some(now);
            task.output_path = Some(path.clone());
            task.format_label = outcome.label.clone();
            task.title = outcome.metadata.title.clone();
            task.thumbnail_url = outcome
                .metadata
                .thumbnail_url
                .clone()
                .or_else(|| task.thumbnail_url.clone());
            task.progress.stage = DownloadStage::Done;
            task.progress.percent = Some(100.0);
            task.progress.speed_bps = 0.0;
            task.progress.eta_sec = None;
        });

        let entry = HistoryEntry {
            id: 0,
            url: outcome.metadata.canonical_url.clone(),
            title: outcome.metadata.title.clone(),
            platform: outcome.metadata.platform,
            thumbnail_url: outcome.metadata.thumbnail_url.clone(),
            file_path: path.clone(),
            file_exists: true,
            container: outcome.container.clone(),
            quality_label: outcome.quality_label.clone(),
            file_size: Some(outcome.file_size),
            created_at: now,
            status: "completed".to_string(),
            request: task.as_ref().map(|task| task.request.clone()),
        };
        if let Err(err) = self.db.history_insert(&entry) {
            log_error!("queue", "could not write history: {err}");
        }

        downloader::cleanup_task_files(id);
        self.persist();
        self.emit_task(id);
        self.emit_changed();
        crate::notify::download_complete(&self.app, &self.settings(), &entry);
    }

    fn fail(self: &Arc<Self>, id: &str, err: AppError) {
        let info = err.to_info();
        log_error!("queue", "task {id} failed: {err}");

        let task = self.update_task(id, |task| {
            task.status = DownloadStatus::Failed;
            task.error = Some(info.clone());
            task.completed_at = Some(util::now_ms());
            task.progress.speed_bps = 0.0;
            task.progress.eta_sec = None;
        });

        self.persist();
        self.emit_task(id);
        self.emit_changed();

        if let Some(task) = task {
            crate::notify::download_failed(&self.app, &self.settings(), &task.title, &info);
        }
    }

    /// A cancelled transfer could mean either "pause" or "stop"; the control
    /// flag is what says which.
    fn settle_interruption(self: &Arc<Self>, id: &str, state: ControlState) {
        let paused = state == ControlState::Paused;
        self.update_task(id, |task| {
            task.status = if paused {
                DownloadStatus::Paused
            } else {
                DownloadStatus::Canceled
            };
            task.progress.speed_bps = 0.0;
            task.progress.eta_sec = None;
        });

        if !paused {
            downloader::cleanup_task_files(id);
        }
        self.persist();
        self.emit_task(id);
        self.emit_changed();
    }

    fn on_progress(self: &Arc<Self>, id: &str, progress: DownloadProgress) {
        let updated = self.update_task(id, |task| {
            if task.status == DownloadStatus::Preparing || task.status == DownloadStatus::Downloading
            {
                task.status = match progress.stage {
                    DownloadStage::Merging | DownloadStage::Converting | DownloadStage::Finalizing => {
                        DownloadStatus::Processing
                    }
                    _ => DownloadStatus::Downloading,
                };
            }
            task.progress = progress.clone();
        });

        if let Some(task) = updated {
            // Progress is sent as its own lightweight event rather than a full
            // list refresh, so a running download does not re-render the queue.
            let _ = self.app.emit(
                EVENT_PROGRESS,
                ProgressEvent {
                    id: task.id,
                    status: task.status,
                    progress: task.progress,
                    output_path: task.output_path,
                    error: task.error,
                },
            );
        }
    }

    fn update_task(
        &self,
        id: &str,
        f: impl FnOnce(&mut DownloadTask),
    ) -> Option<DownloadTask> {
        self.with_tasks(|tasks| {
            let task = tasks.iter_mut().find(|task| task.id == id)?;
            f(task);
            Some(task.clone())
        })
    }

    fn control_for(&self, id: &str) -> Option<Arc<TaskControl>> {
        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn persist(&self) {
        let tasks = self.list();
        if let Err(err) = self.db.queue_replace(&tasks) {
            log_error!("queue", "could not persist the queue: {err}");
        }
    }

    fn emit_changed(&self) {
        let _ = self.app.emit(EVENT_CHANGED, self.list());
    }

    fn emit_task(&self, id: &str) {
        let Some(task) = self.with_tasks(|tasks| tasks.iter().find(|t| t.id == id).cloned()) else {
            return;
        };
        let _ = self.app.emit(
            EVENT_PROGRESS,
            ProgressEvent {
                id: task.id,
                status: task.status,
                progress: task.progress,
                output_path: task.output_path,
                error: task.error,
            },
        );
    }

    /// Stop everything on the way out, so no worker writes after shutdown.
    pub fn shutdown(&self) {
        let controls = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        for control in controls.values() {
            control.pause();
        }
    }
}

/// Tasks the user can still reorder: everything not running and not finished.
fn is_waiting(task: &DownloadTask) -> bool {
    matches!(task.status, DownloadStatus::Queued | DownloadStatus::Paused)
}

pub fn require_task(manager: &QueueManager, id: &str) -> AppResult<DownloadTask> {
    manager
        .list()
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| AppError::Other(format!("no queued download with id {id}")))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DownloadMode, QualityPreference, WatermarkPreference};

    fn task(id: &str, status: DownloadStatus) -> DownloadTask {
        DownloadTask {
            id: id.to_string(),
            url: "https://example.test/x".into(),
            title: id.to_string(),
            platform: crate::model::PlatformId::Generic,
            thumbnail_url: None,
            status,
            progress: DownloadProgress::default(),
            format_label: "Best".into(),
            output_path: None,
            error: None,
            created_at: 0,
            started_at: None,
            completed_at: None,
            attempt: 0,
            request: DownloadRequest {
                url: "https://example.test/x".into(),
                mode: DownloadMode::Video,
                quality: QualityPreference::Best,
                video_format_id: None,
                audio_format_id: None,
                container: None,
                watermark: WatermarkPreference::Any,
                output_dir: None,
                title: None,
                thumbnail_url: None,
                platform: None,
            },
        }
    }

    /// Mirrors `QueueManager::reorder`'s inner logic on a plain vector, so the
    /// ordering rules can be checked without an `AppHandle`.
    fn reorder(tasks: &mut [DownloadTask], id: &str, delta: i32) -> bool {
        let Some(index) = tasks.iter().position(|task| task.id == id) else {
            return false;
        };
        if !is_waiting(&tasks[index]) {
            return false;
        }
        let step: i32 = if delta < 0 { -1 } else { 1 };
        let mut cursor = index as i32 + step;
        while cursor >= 0 && (cursor as usize) < tasks.len() {
            if is_waiting(&tasks[cursor as usize]) {
                tasks.swap(index, cursor as usize);
                return true;
            }
            cursor += step;
        }
        false
    }

    fn ids(tasks: &[DownloadTask]) -> Vec<&str> {
        tasks.iter().map(|task| task.id.as_str()).collect()
    }

    #[test]
    fn moving_up_swaps_with_the_previous_waiting_task() {
        let mut tasks = vec![
            task("a", DownloadStatus::Queued),
            task("b", DownloadStatus::Queued),
        ];
        assert!(reorder(&mut tasks, "b", -1));
        assert_eq!(ids(&tasks), vec!["b", "a"]);
    }

    #[test]
    fn reordering_skips_over_running_and_finished_tasks() {
        // Without the skip this would swap "c" with the running task and the
        // grouped list would look unchanged.
        let mut tasks = vec![
            task("a", DownloadStatus::Queued),
            task("running", DownloadStatus::Downloading),
            task("done", DownloadStatus::Completed),
            task("c", DownloadStatus::Queued),
        ];
        assert!(reorder(&mut tasks, "c", -1));
        assert_eq!(ids(&tasks), vec!["c", "running", "done", "a"]);
    }

    #[test]
    fn a_task_at_the_edge_of_the_queue_does_not_move() {
        let mut tasks = vec![
            task("a", DownloadStatus::Queued),
            task("running", DownloadStatus::Downloading),
        ];
        assert!(!reorder(&mut tasks, "a", -1));
        assert!(!reorder(&mut tasks, "a", 1));
        assert_eq!(ids(&tasks), vec!["a", "running"]);
    }

    #[test]
    fn a_running_task_cannot_be_reordered() {
        let mut tasks = vec![
            task("a", DownloadStatus::Queued),
            task("running", DownloadStatus::Downloading),
            task("b", DownloadStatus::Queued),
        ];
        assert!(!reorder(&mut tasks, "running", -1));
        assert_eq!(ids(&tasks), vec!["a", "running", "b"]);
    }

    #[test]
    fn paused_tasks_are_reorderable_alongside_queued_ones() {
        let mut tasks = vec![
            task("a", DownloadStatus::Queued),
            task("b", DownloadStatus::Paused),
        ];
        assert!(reorder(&mut tasks, "b", -1));
        assert_eq!(ids(&tasks), vec!["b", "a"]);
    }
}

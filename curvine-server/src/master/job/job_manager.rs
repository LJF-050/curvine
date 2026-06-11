// Copyright 2025 OPPO.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::common::UfsFactory;
use crate::master::fs::MasterFilesystem;
use crate::master::{JobStore, LoadJobRunner, MountManager};
use core::time::Duration;
use curvine_client::unified::MountValue;
use curvine_common::conf::ClusterConf;
use curvine_common::error::FsError;
use curvine_common::executor::ScheduledExecutor;
use curvine_common::fs::Path;
use curvine_common::state::{
    JobStatus, JobTaskProgress, JobTaskState, LoadJobCommand, LoadJobResult, LoadTaskStatus,
};
use curvine_common::FsResult;
use log::{debug, info};
use once_cell::sync::Lazy;
use orpc::common::{Counter, Gauge, LocalTime, Metrics as m};
use orpc::runtime::{LoopTask, Runtime};
use orpc::{err_box, err_ext};
use std::sync::Arc;
use tokio::time::timeout;

struct LoadJobMetrics {
    submitted: Counter,
    completed: Counter,
    failed: Counter,
    canceled: Counter,
    failed_tasks: Counter,
    running_jobs: Gauge,
    running_tasks: Gauge,
}

static LOAD_JOB_METRICS: Lazy<LoadJobMetrics> = Lazy::new(|| LoadJobMetrics {
    submitted: m::new_counter("load_jobs_submitted_total", "Submitted load/export jobs")
        .expect("register load job submitted metric"),
    completed: m::new_counter("load_jobs_completed_total", "Completed load/export jobs")
        .expect("register load job completed metric"),
    failed: m::new_counter("load_jobs_failed_total", "Failed load/export jobs")
        .expect("register load job failed metric"),
    canceled: m::new_counter("load_jobs_canceled_total", "Canceled load/export jobs")
        .expect("register load job canceled metric"),
    failed_tasks: m::new_counter("load_tasks_failed_total", "Failed load/export tasks")
        .expect("register load task failed metric"),
    running_jobs: m::new_gauge("load_jobs_running", "Current running load/export jobs")
        .expect("register load job running metric"),
    running_tasks: m::new_gauge("load_tasks_running", "Current running load/export tasks")
        .expect("register load task running metric"),
});

/// Load the Task Manager
pub struct JobManager {
    rt: Arc<Runtime>,
    jobs: JobStore,
    master_fs: MasterFilesystem,
    factory: Arc<UfsFactory>,
    mount_manager: Arc<MountManager>,
    job_life_ttl: Duration,
    job_cleanup_ttl: Duration,
    job_max_files: usize,
}

impl JobManager {
    pub fn from_cluster_conf(
        master_fs: MasterFilesystem,
        mount_manager: Arc<MountManager>,
        rt: Arc<Runtime>,
        conf: &ClusterConf,
    ) -> Self {
        let factory = Arc::new(UfsFactory::with_rt(&conf.client, rt.clone()));

        Self {
            rt,
            jobs: JobStore::new(),
            master_fs,
            factory,
            mount_manager,
            job_life_ttl: conf.job.job_life_ttl,
            job_cleanup_ttl: conf.job.job_cleanup_ttl,
            job_max_files: conf.job.job_max_files,
        }
    }

    /// Start the job manager
    pub fn start(&self) {
        let cleanup_interval = self.job_cleanup_ttl.as_millis() as u64;
        let ttl_ms = self.job_life_ttl.as_millis() as i64;

        let executor = ScheduledExecutor::new("job_cleanup", cleanup_interval);
        executor
            .start(JobCleanupTask {
                jobs: self.jobs.clone(),
                ttl_ms,
            })
            .unwrap();

        info!("JobManager started");
    }

    fn update_state(&self, job_id: &str, state: JobTaskState, message: impl Into<String>) {
        let previous = self.jobs.get(job_id).map(|job| job.state.state());
        self.jobs.update_state(job_id, state, message);
        self.record_terminal_transition(previous, state);
        self.refresh_running_metrics();
    }

    fn record_terminal_transition(&self, previous: Option<JobTaskState>, next: JobTaskState) {
        if previous.is_some_and(|state| state.is_finish()) || !next.is_finish() {
            return;
        }
        match next {
            JobTaskState::Completed => LOAD_JOB_METRICS.completed.inc(),
            JobTaskState::Failed => LOAD_JOB_METRICS.failed.inc(),
            JobTaskState::Canceled => LOAD_JOB_METRICS.canceled.inc(),
            _ => {}
        }
    }

    fn refresh_running_metrics(&self) {
        let mut running_jobs = 0i64;
        let mut running_tasks = 0i64;
        for entry in self.jobs.iter() {
            let status = Self::job_status_from_context(entry.value(), false);
            if status.state.is_running() {
                running_jobs += 1;
            }
            running_tasks += status.running_files as i64;
        }
        LOAD_JOB_METRICS.running_jobs.set(running_jobs);
        LOAD_JOB_METRICS.running_tasks.set(running_tasks);
    }

    pub async fn wait_job_complete(
        &self,
        job_id: impl AsRef<str>,
        duration: Duration,
    ) -> FsResult<JobStatus> {
        timeout(duration, self.wait_job_complete0(job_id)).await?
    }

    async fn wait_job_complete0(&self, job_id: impl AsRef<str>) -> FsResult<JobStatus> {
        let job_id = job_id.as_ref();

        let mut listener = match self.jobs.get(job_id) {
            Some(job) => job.new_listener(),
            None => return err_ext!(FsError::job_not_found(job_id)),
        };

        let status = self.get_job_status(job_id)?;
        if status.state.is_finish() {
            return Ok(status);
        }

        loop {
            let next_state = JobTaskState::from(listener.next_state().await?);
            if next_state.is_finish() {
                return self.get_job_status(job_id);
            }
        }
    }

    fn job_status_from_context(job: &crate::master::JobContext, include_tasks: bool) -> JobStatus {
        let mut completed_files = 0u64;
        let mut failed_files = 0u64;
        let mut running_files = 0u64;
        let mut pending_files = 0u64;
        let mut loading_files = 0u64;
        let mut tasks = Vec::new();

        for detail in job.tasks.values() {
            match detail.progress.state {
                JobTaskState::Completed => completed_files += 1,
                JobTaskState::Failed => failed_files += 1,
                JobTaskState::Pending => {
                    pending_files += 1;
                    running_files += 1;
                }
                JobTaskState::Loading => {
                    loading_files += 1;
                    running_files += 1;
                }
                _ => {}
            }
            if include_tasks {
                tasks.push(LoadTaskStatus {
                    task_id: detail.task.task_id.clone(),
                    source_path: detail.task.source_path.clone(),
                    target_path: detail.task.target_path.clone(),
                    worker: detail.task.worker.to_string(),
                    state: detail.progress.state,
                    progress: detail.progress.clone(),
                    create_time: detail.task.create_time,
                    update_time: detail.progress.update_time,
                });
            }
        }
        let total_files = job.tasks.len() as u64;
        tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));

        JobStatus {
            job_id: job.info.job_id.clone(),
            state: job.state.state(),
            source_path: job.info.source_path.clone(),
            target_path: job.info.target_path.clone(),
            progress: job.progress.clone(),
            total_files,
            completed_files,
            failed_files,
            running_files,
            pending_files,
            loading_files,
            source_type: job.info.source_type,
            tasks,
        }
    }

    pub fn get_job_status(&self, job_id: impl AsRef<str>) -> FsResult<JobStatus> {
        self.get_job_status_with_options(job_id, false, false)
    }

    pub fn get_job_status_with_options(
        &self,
        job_id: impl AsRef<str>,
        include_tasks: bool,
        failed_only: bool,
    ) -> FsResult<JobStatus> {
        let job_id = job_id.as_ref();
        if let Some(job) = self.jobs.get(job_id) {
            let mut status = Self::job_status_from_context(job.value(), include_tasks);
            if failed_only {
                retain_failure_context(&mut status);
            }
            Ok(status)
        } else {
            err_ext!(FsError::job_not_found(job_id))
        }
    }

    pub fn list_job_statuses(
        &self,
        path_prefix: Option<&str>,
        limit: usize,
        include_finished: bool,
    ) -> Vec<JobStatus> {
        self.list_job_statuses_filtered(
            path_prefix,
            limit,
            include_finished,
            None,
            None,
            false,
            false,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_job_statuses_filtered(
        &self,
        path_prefix: Option<&str>,
        limit: usize,
        include_finished: bool,
        state_filter: Option<JobTaskState>,
        mount_path: Option<&str>,
        include_tasks: bool,
        failed_only: bool,
        offset: usize,
    ) -> Vec<JobStatus> {
        let mut statuses = Vec::new();
        let limit = limit.clamp(1, 500);
        for entry in self.jobs.iter() {
            let job = entry.value();
            let state: JobTaskState = job.state.state();
            if !include_finished && state.is_finish() {
                continue;
            }
            if state_filter.is_some_and(|filter| filter != state) {
                continue;
            }
            if let Some(mount) = mount_path.filter(|value| !value.is_empty()) {
                let cv_path = job.info.mount_info.cv_path.trim_end_matches('/');
                let normalized = mount.trim_end_matches('/');
                if cv_path != normalized {
                    continue;
                }
            }
            if let Some(prefix) = path_prefix.filter(|value| !value.is_empty()) {
                let matches_prefix = |path: &str| {
                    prefix == "/"
                        || path == prefix
                        || path.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
                };
                if !matches_prefix(&job.info.source_path) && !matches_prefix(&job.info.target_path)
                {
                    continue;
                }
            }
            let mut status = Self::job_status_from_context(job, include_tasks);
            if failed_only {
                // A job is "failed" for diagnostic purposes if any task failed OR
                // the job itself reached the Failed state (e.g. a dispatch failure
                // where tasks never left Pending). Excluding the latter would hide
                // exactly the incidents operators need to investigate.
                if status.failed_files == 0 && status.state != JobTaskState::Failed {
                    continue;
                }
                retain_failure_context(&mut status);
            }
            statuses.push(status);
        }
        statuses.sort_by(|a, b| b.progress.update_time.cmp(&a.progress.update_time));
        if offset > 0 {
            statuses.drain(..offset.min(statuses.len()));
        }
        statuses.truncate(limit);
        statuses
    }

    pub fn create_runner(&self) -> LoadJobRunner {
        LoadJobRunner::new(
            self.jobs.clone(),
            self.master_fs.clone(),
            self.factory.clone(),
            self.job_max_files,
        )
    }

    pub fn get_mnt(&self, path: &Path) -> FsResult<Option<(Path, Arc<MountValue>)>> {
        if let Some(mnt) = self.mount_manager.get_mount_info(path)? {
            let mnt_value = self.factory.get_mnt(&mnt)?;
            let target_path = mnt_value.toggle_path(path)?;

            Ok(Some((target_path, mnt_value)))
        } else {
            Ok(None)
        }
    }

    pub fn rt(&self) -> &Runtime {
        &self.rt
    }

    /// See `LoadJobRunner::submit_load_task` for the concurrency contract: concurrent
    /// submits for the same path while a load is running return the **existing** run’s
    /// result; the new command’s options are not applied (first submitter wins).
    pub async fn submit_load_job(&self, command: LoadJobCommand) -> FsResult<LoadJobResult> {
        let source_path = Path::from_str(&command.source_path)?;

        // Check mount info for both UFS and CV paths
        // - For UFS path: Import (UFS → Curvine)
        // - For CV path: Export (Curvine → UFS), requires mount info to determine target UFS
        let mnt = if let Some(mnt) = self.mount_manager.get_mount_info(&source_path)? {
            mnt
        } else {
            return err_box!("Not found mount info for path: {}", source_path);
        };

        let job_runner = self.create_runner();
        let result = job_runner.submit_load_task(command, mnt).await;
        if result.is_ok() {
            self.record_submitted();
        }
        result
    }

    /// Record a successfully submitted job in the metrics. Invoked by both the
    /// manual submit path and the fs_mode auto-export path (`UfsLoader`) so that
    /// `load_jobs_submitted_total` reflects every origin, not just manual jobs.
    pub fn record_submitted(&self) {
        LOAD_JOB_METRICS.submitted.inc();
        self.refresh_running_metrics();
    }

    /// Handle cancellation of tasks
    pub async fn cancel_job(&self, job_id: impl AsRef<str>) -> FsResult<()> {
        let job_id = job_id.as_ref();
        let assigned_workers = {
            if let Some(job) = self.jobs.get(job_id) {
                let state: JobTaskState = job.state.state();
                // Check whether it can be canceled
                if state == JobTaskState::Completed
                    || state == JobTaskState::Failed
                    || state == JobTaskState::Canceled
                {
                    info!(
                        "job {} is already in final state {:?}, source_path: {}, target_path: {}",
                        job_id, state, job.info.source_path, job.info.target_path
                    );
                    self.update_state(job_id, JobTaskState::Canceled, "Canceling job by user");
                    return Ok(());
                }

                job.assigned_workers.clone()
            } else {
                return err_ext!(FsError::job_not_found(job_id));
            }
        };

        self.update_state(job_id, JobTaskState::Canceled, "Canceling job by user");

        let job_runner = self.create_runner();
        job_runner.cancel_job(&job_id, assigned_workers).await?;

        Ok(())
    }

    pub fn update_progress(
        &self,
        job_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
        progress: JobTaskProgress,
    ) -> FsResult<()> {
        let job_id = job_id.as_ref();
        let previous = self.jobs.get(job_id).map(|job| job.state.state());
        let task_failed = progress.state == JobTaskState::Failed;
        self.jobs.update_progress(job_id, task_id, progress)?;
        let next = self.jobs.get(job_id).map(|job| job.state.state());
        if task_failed {
            LOAD_JOB_METRICS.failed_tasks.inc();
        }
        if let Some(next) = next {
            self.record_terminal_transition(previous, next);
        }
        self.refresh_running_metrics();
        Ok(())
    }

    pub fn jobs(&self) -> &JobStore {
        &self.jobs
    }

    pub fn factory(&self) -> &Arc<UfsFactory> {
        &self.factory
    }
}

/// Reduce a job's task list to the failure-relevant subset for `failed_only`
/// queries. If any task actually failed, keep only the failed tasks. If the job
/// failed without any per-task failure (e.g. a dispatch failure that left tasks
/// Pending), keep all tasks so the failure context is still visible instead of
/// returning an empty, misleading task list.
fn retain_failure_context(status: &mut JobStatus) {
    if status.tasks.iter().any(|task| task.state == JobTaskState::Failed) {
        status.tasks.retain(|task| task.state == JobTaskState::Failed);
    }
}

struct JobCleanupTask {
    jobs: JobStore,
    ttl_ms: i64,
}

impl LoopTask for JobCleanupTask {
    type Error = FsError;

    fn run(&self) -> Result<(), Self::Error> {
        // Collect tasks that need to be removed first
        let mut jobs_to_remove = vec![];
        let now = LocalTime::mills() as i64;
        for entry in self.jobs.iter() {
            let job = entry.value();
            // Never evict a job that is still pending/loading: a long-running job
            // must not disappear mid-flight, and operators rely on it being visible.
            let state: JobTaskState = job.state.state();
            if !state.is_finish() {
                continue;
            }
            // Retain finished jobs for `ttl_ms` after they reached a terminal
            // state (last progress update), not after creation, so that failed
            // jobs stay queryable long enough for incident investigation.
            let finished_at = if job.progress.update_time > 0 {
                job.progress.update_time
            } else {
                job.info.create_time
            };
            if now > self.ttl_ms + finished_at {
                jobs_to_remove.push(job.info.job_id.clone());
            }
        }

        for job_id in jobs_to_remove {
            if let Some(v) = self.jobs.remove(&job_id) {
                debug!("Removing expired job: {:?}", v.1.info);
            }
        }

        Ok(())
    }

    fn terminate(&self) -> bool {
        false
    }
}

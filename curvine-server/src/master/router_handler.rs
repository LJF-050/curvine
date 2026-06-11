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

use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{timeout, Duration};

use axum::body::Body;
use axum::extract::{Path, Query, Request};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use curvine_client::rpc::{JobListOptions, JobMasterClient};
use curvine_client::unified::{UfsFileSystem, UnifiedFileSystem};
use curvine_common::conf::ClusterConf;
use curvine_common::error::{ErrorKind, FsError};
use curvine_common::fs::{FileSystem, Path as FsPath, Reader, Writer};
use curvine_common::state::{
    FileBlocks, FileStatus, JobSourceType, JobStatus, JobTaskState, LoadJobCommand, MountOptions,
    Provider, StorageState, StorageType, TtlAction, WorkerInfo, WorkerStatus, WriteType,
};
use curvine_common::utils::CommonUtils;
use curvine_common::FsResult;
use curvine_web::router::RouterHandler;
use orpc::common::{ByteUnit, DurationUnit, LocalTime};
use orpc::err_box;
use orpc::runtime::Runtime;

use crate::master::fs::MasterFilesystem;
use crate::master::Master;

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct FsPathRequest {
    path: String,
    #[serde(default)]
    create_parent: Option<bool>,
    #[serde(default)]
    recursive: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct WorkerActionRequest {
    worker: String,
    action: String,
}

#[derive(Debug, Deserialize)]
struct MountRequest {
    cv_path: String,
    ufs_path: String,
    #[serde(default)]
    update: bool,
    #[serde(default = "default_write_type")]
    write_type: String,
    #[serde(default = "default_ttl")]
    ttl: String,
    #[serde(default)]
    read_verify_ufs: bool,
    #[serde(default)]
    replicas: Option<i32>,
    #[serde(default)]
    block_size: Option<String>,
    #[serde(default)]
    storage_type: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    properties: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MountPathRequest {
    cv_path: String,
}

#[derive(Debug, Deserialize)]
struct MountValidateRequest {
    ufs_path: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    properties: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MountResyncRequest {
    cv_path: String,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct SubmitLoadRequest {
    path: String,
    #[serde(default)]
    target_path: Option<String>,
    #[serde(default)]
    ttl: Option<String>,
    #[serde(default)]
    ttl_action: Option<String>,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    configs: HashMap<String, String>,
    #[serde(default)]
    replicas: Option<i32>,
    #[serde(default)]
    block_size: Option<String>,
    #[serde(default)]
    storage_type: Option<String>,
    #[serde(default)]
    overwrite: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct JobListQuery {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct UfsSyncJobsQuery {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    include_finished: Option<bool>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    include_tasks: Option<bool>,
    #[serde(default)]
    failed_only: Option<bool>,
    #[serde(default)]
    offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct ResyncTaskState {
    id: String,
    cv_path: String,
    mount_path: String,
    write_type: String,
    status: String,
    scanned: usize,
    skipped: usize,
    recreated: usize,
    failed: usize,
    pending_dirs: usize,
    message: String,
    started_at: String,
    updated_at: String,
    done: bool,
    dry_run: bool,
}

fn default_write_type() -> String {
    "fs_mode".to_string()
}

fn default_ttl() -> String {
    "7d".to_string()
}

fn normalize_ufs_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        format!("file://{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

fn worker_matches(worker: &WorkerInfo, selector: &str) -> bool {
    let selector = selector.trim();
    if selector.contains(':') {
        return worker.address.connect_addr() == selector;
    }
    worker.worker_id().to_string() == selector
        || worker.address.worker_id.to_string() == selector
        || worker.address.hostname == selector
        || worker.address.ip_addr == selector
}

fn worker_payload(worker: WorkerInfo, state: &str) -> Value {
    json!({
        "worker": worker,
        "state": state,
        "usage_percent": if worker.capacity > 0 {
            ((worker.capacity - worker.available) as f64 / worker.capacity as f64 * 100.0).round()
        } else {
            0.0
        },
        "storage_count": worker.storage_map.len(),
        "version": Value::Null,
        "file_count": Value::Null
    })
}

#[derive(Clone)]
pub struct MasterRouterHandler {
    fs: MasterFilesystem,
    conf: ClusterConf,
    unified_fs: Option<UnifiedFileSystem>,
    start_time: String,
    resync_tasks: Arc<Mutex<HashMap<String, ResyncTaskState>>>,
    load_jobs: Arc<Mutex<VecDeque<String>>>,
    load_job_cache: Arc<Mutex<HashMap<String, Value>>>,
    auth_sessions: Arc<Mutex<HashMap<String, i64>>>,
}

const AUTH_COOKIE_NAME: &str = "curvine_web_session";
const DEFAULT_AUTH_SESSION_TTL_SECS: i64 = 12 * 60 * 60;
const DEFAULT_AUTH_PASSWORD_FILE: &str = "data/web-password";

fn auth_username() -> String {
    env::var("CURVINE_WEB_USERNAME").unwrap_or_else(|_| "admin".to_string())
}

fn auth_password() -> Option<String> {
    if let Ok(password) = env::var("CURVINE_WEB_PASSWORD") {
        let password = password.trim().to_string();
        if !password.is_empty() {
            return Some(password);
        }
    }

    let password_file = env::var("CURVINE_WEB_PASSWORD_FILE")
        .unwrap_or_else(|_| DEFAULT_AUTH_PASSWORD_FILE.to_string());
    std::fs::read_to_string(password_file)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn auth_session_ttl_secs() -> i64 {
    env::var("CURVINE_WEB_SESSION_TTL_SECS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_AUTH_SESSION_TTL_SECS)
}

fn auth_cookie_secure() -> bool {
    env::var("CURVINE_WEB_COOKIE_SECURE")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut diff = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or_default();
        let b = right.get(index).copied().unwrap_or_default();
        diff |= (a ^ b) as usize;
    }
    diff == 0
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

fn new_session_token() -> String {
    Uuid::new_v4().to_string()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    for item in cookies.split(';') {
        let trimmed = item.trim();
        if let Some((key, value)) = trimmed.split_once('=') {
            if key == name {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn session_cookie(token: &str) -> String {
    format!(
        "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
        AUTH_COOKIE_NAME,
        token,
        auth_session_ttl_secs(),
        if auth_cookie_secure() { "; Secure" } else { "" }
    )
}

fn expired_session_cookie() -> String {
    format!(
        "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax",
        AUTH_COOKIE_NAME
    )
}

fn is_authenticated(instance: &MasterRouterHandler, headers: &HeaderMap) -> bool {
    let Some(token) = cookie_value(headers, AUTH_COOKIE_NAME) else {
        return false;
    };
    let now = now_secs();
    match instance.auth_sessions.lock() {
        Ok(mut sessions) => {
            sessions.retain(|_, expires_at| *expires_at > now);
            sessions
                .get(&token)
                .map(|expires_at| *expires_at > now)
                .unwrap_or(false)
        }
        Err(_) => false,
    }
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "success": false,
            "data": null,
            "error": { "code": "UNAUTHORIZED", "message": "login required" }
        })),
    )
        .into_response()
}

/// Whether a request path must carry a valid session before it is served.
///
/// Auth endpoints stay open so the UI can authenticate. Every data-bearing API
/// is protected, including the legacy non-versioned routes and the
/// human-readable `/report`, which previously leaked filesystem listings and
/// full cluster config to anonymous callers. `/metrics` is intentionally left
/// open for Prometheus scraping; restrict it at the network layer if needed.
fn path_requires_auth(path: &str) -> bool {
    if path.starts_with("/api/v1/auth") {
        return false;
    }
    path.starts_with("/api/") || path == "/report"
}

async fn require_auth(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if !path_requires_auth(path) || is_authenticated(&instance, &headers) {
        return next.run(request).await;
    }
    unauthorized_response()
}

fn api_success(data: Value) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": data,
        "error": null
    }))
}

fn new_task_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    format!("resync-{millis}")
}

fn update_resync_task<F>(
    tasks: &Arc<Mutex<HashMap<String, ResyncTaskState>>>,
    task_id: &str,
    updater: F,
) where
    F: FnOnce(&mut ResyncTaskState),
{
    if let Ok(mut map) = tasks.lock() {
        if let Some(task) = map.get_mut(task_id) {
            updater(task);
            task.updated_at = LocalTime::now_datetime();
        }
    }
}

fn is_cv_dir_missing(err: &FsError) -> bool {
    matches!(err.kind(), ErrorKind::FileNotFound | ErrorKind::Expired)
}

fn job_state_name(state: JobTaskState) -> &'static str {
    match state {
        JobTaskState::Pending => "Pending",
        JobTaskState::Loading => "Loading",
        JobTaskState::Completed => "Completed",
        JobTaskState::Failed => "Failed",
        JobTaskState::Canceled => "Canceled",
        JobTaskState::UNKNOWN => "Unknown",
    }
}

fn parse_job_state(value: &str) -> Option<JobTaskState> {
    match value.to_ascii_lowercase().as_str() {
        "pending" => Some(JobTaskState::Pending),
        "loading" | "running" => Some(JobTaskState::Loading),
        "completed" | "complete" | "done" => Some(JobTaskState::Completed),
        "failed" | "fail" => Some(JobTaskState::Failed),
        "canceled" | "cancelled" | "cancel" => Some(JobTaskState::Canceled),
        "unknown" => Some(JobTaskState::UNKNOWN),
        _ => None,
    }
}

fn job_progress_percent(status: &JobStatus) -> f64 {
    if status.progress.total_size > 0 {
        (status.progress.loaded_size as f64 / status.progress.total_size as f64 * 100.0)
            .clamp(0.0, 100.0)
    } else if status.state == JobTaskState::Completed {
        100.0
    } else {
        0.0
    }
}

fn job_status_payload(status: JobStatus) -> Value {
    let progress = job_progress_percent(&status);
    let mut total_files = status.total_files;
    let mut completed_files = status.completed_files;
    let mut failed_files = status.failed_files;
    let mut running_files = status.running_files;
    if total_files == 0 && status.progress.total_size > 0 {
        total_files = 1;
        if status.state == JobTaskState::Completed {
            completed_files = 1;
        } else if status.state == JobTaskState::Failed {
            failed_files = 1;
        } else if !status.state.is_finish() {
            running_files = 1;
        }
    }
    json!({
        "job_id": status.job_id,
        "id": status.job_id,
        "path": status.source_path,
        "source_path": status.source_path,
        "target_path": status.target_path,
        "state": job_state_name(status.state),
        "status": job_state_name(status.state),
        "message": status.progress.message,
        "total_size": status.progress.total_size,
        "loaded_size": status.progress.loaded_size,
        "progress": progress,
        "total_files": total_files,
        "completed_files": completed_files,
        "failed_files": failed_files,
        "running_files": running_files,
        "pending_files": status.pending_files,
        "loading_files": status.loading_files,
        "source_type": status.source_type.as_str(),
        "created_by": match status.source_type {
            JobSourceType::Manual => "User",
            JobSourceType::FsModeAuto => "UfsLoader",
        },
        "trigger_event": match status.source_type {
            JobSourceType::Manual => "Manual",
            JobSourceType::FsModeAuto => "CompleteFile / Rename",
        },
        "tasks": status.tasks.into_iter().map(|task| json!({
            "task_id": task.task_id,
            "source_path": task.source_path,
            "target_path": task.target_path,
            "worker": task.worker,
            "state": job_state_name(task.state),
            "status": job_state_name(task.state),
            "message": task.progress.message,
            "total_size": task.progress.total_size,
            "loaded_size": task.progress.loaded_size,
            "update_time_ms": task.update_time,
            "create_time_ms": task.create_time,
        })).collect::<Vec<_>>(),
        "update_time_ms": status.progress.update_time,
        "done": status.state.is_finish(),
    })
}

fn remember_load_job(history: &Arc<Mutex<VecDeque<String>>>, job_id: String) {
    if let Ok(mut jobs) = history.lock() {
        if let Some(index) = jobs.iter().position(|item| item == &job_id) {
            jobs.remove(index);
        }
        jobs.push_front(job_id);
        while jobs.len() > 100 {
            jobs.pop_back();
        }
    }
}

fn is_cv_to_ufs_sync(source_path: &str, target_path: &str) -> bool {
    let Ok(source) = FsPath::from_str(source_path) else {
        return false;
    };
    let Ok(target) = FsPath::from_str(target_path) else {
        return false;
    };
    source.is_cv() && !target.is_cv()
}

fn is_cv_to_ufs_sync_payload(payload: &Value) -> bool {
    let source_path = payload
        .get("source_path")
        .or_else(|| payload.get("path"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let target_path = payload
        .get("target_path")
        .or_else(|| payload.get("ufs_path"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    is_cv_to_ufs_sync(source_path, target_path)
}

fn cache_load_job(cache: &Arc<Mutex<HashMap<String, Value>>>, job_id: &str, payload: Value) {
    if let Ok(mut map) = cache.lock() {
        map.insert(job_id.to_string(), payload);
    }
}

async fn with_job_timeout<T>(
    future: impl std::future::Future<Output = FsResult<T>>,
) -> FsResult<T> {
    match timeout(Duration::from_secs(5), future).await {
        Ok(result) => result,
        Err(_) => err_box!("job request timed out"),
    }
}

impl MasterRouterHandler {
    pub fn new(conf: ClusterConf, fs: MasterFilesystem) -> Self {
        Self::with_rt(conf, fs, Arc::new(Runtime::single()))
    }

    pub fn with_rt(conf: ClusterConf, fs: MasterFilesystem, rt: Arc<Runtime>) -> Self {
        let unified_fs = match UnifiedFileSystem::with_rt(conf.clone(), rt) {
            Ok(client) => Some(client),
            Err(e) => {
                log::warn!("failed to initialize unified fs for web api: {}", e);
                None
            }
        };

        Self {
            fs,
            conf,
            unified_fs,
            start_time: LocalTime::now_datetime(),
            resync_tasks: Arc::new(Mutex::new(HashMap::new())),
            load_jobs: Arc::new(Mutex::new(VecDeque::new())),
            load_job_cache: Arc::new(Mutex::new(HashMap::new())),
            auth_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn unified_client(&self, cache_only: bool) -> Option<UnifiedFileSystem> {
        self.unified_fs.clone().map(|mut client| {
            if cache_only {
                client.disable_unified();
            }
            client
        })
    }

    fn job_client(&self) -> FsResult<JobMasterClient> {
        match self.unified_client(false) {
            Some(client) => Ok(JobMasterClient::new(client.fs_client())),
            None => err_box!("job client is unavailable"),
        }
    }
}

fn get_report(fs: MasterFilesystem) -> HashMap<String, String> {
    let metrics = Master::get_metrics();
    let output = metrics.text_output(fs).unwrap_or_default();
    let report = output
        .lines()
        .filter(|line| !line.starts_with("#"))
        .map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next().unwrap();
            let value = parts.next().unwrap();
            let value = match name {
                "capacity" | "available" | "fs_used" => {
                    let v = value.parse::<f64>().unwrap_or(0.0);
                    let v = v / 1024.0 / 1024.0 / 1024.0;
                    format!("{:.2}GB", v)
                }
                _ => value.to_string(),
            };
            let name = name.to_string();
            (name, value)
        })
        .collect();
    report
}

async fn metrics(Extension(instance): Extension<Arc<MasterRouterHandler>>) -> String {
    let metrics = Master::get_metrics();
    metrics.text_output(instance.fs.clone()).unwrap_or_default()
}

async fn report(Extension(instance): Extension<Arc<MasterRouterHandler>>) -> String {
    let report = get_report(instance.fs.clone());
    let available = &report.get("available").unwrap();
    let capacity = &report.get("capacity").unwrap();
    let fs_used = &report.get("fs_used").unwrap();
    let dir_total = &report.get("inode_dir_num").unwrap();
    let files_total = &report.get("inode_file_num").unwrap();
    let live_workers = &report.get("live_workers").unwrap();
    let lost_workers = &report.get("lost_workers").unwrap();

    let result = format!(
        r#"Curvine cluster summary:
    available: {available}
    capacity: {capacity}
    fs_used: {fs_used}
    dir_total: {dir_total}
    files_total: {files_total}
    live_workers: {live_workers}
    lost_workers: {lost_workers}
    "#
    );
    result
}

async fn overview(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    let fs = &instance.fs;
    let conf = &instance.conf;
    let start_time = &instance.start_time;
    let master_info = fs.master_info()?;

    let (dir_total, files_total) = {
        let (dir_count, file_count) = fs.get_file_counts();
        (dir_count.max(0), file_count.max(0))
    };

    let expected_capacity = master_info.available + master_info.fs_used;
    if master_info.capacity != expected_capacity {
        log::warn!(
            "Capacity inconsistency detected: capacity={}, available={}, fs_used={}, expected_capacity={}",
            master_info.capacity,
            master_info.available,
            master_info.fs_used,
            expected_capacity
        );
    } else {
        log::debug!(
            "Capacity consistency verified: capacity={}, available={}, fs_used={}",
            master_info.capacity,
            master_info.available,
            master_info.fs_used
        );
    }

    let master_state = format!("{:?}", fs.master_monitor.journal_state());

    let res = Json(json!({
        "cluster_id": conf.cluster_id,
        "master_addr": conf.master_addr().to_string(),
        "start_time": start_time,
        "live_workers": master_info.live_workers.len(),
        "lost_workers": master_info.lost_workers.len(),
        "available": master_info.available,
        "capacity": master_info.capacity,
        "fs_used": master_info.fs_used,
        "reserved_bytes": master_info.reserved_bytes,
        "files_total": files_total,
        "dir_total": dir_total,
        "master_state": master_state,
    }));
    Ok(res)
}

async fn browse(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Vec<FileStatus>>> {
    let fs = &instance.fs;
    let root_path = fs.fs_dir.read().root_dir().name().to_string();
    let path = params.get("path").unwrap_or(&root_path);
    let files = fs.list_status(path)?;
    Ok(Json(files))
}

async fn block_locations(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<FileBlocks>> {
    let fs = &instance.fs;
    let path = match params.get("path") {
        Some(path) => path,
        None => return err_box!("not found path"),
    };
    let files = fs.get_block_locations(path)?;
    Ok(Json(files))
}

async fn workers(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<HashMap<String, Vec<WorkerInfo>>>> {
    let fs = &instance.fs;
    let wm = fs.worker_manager.read();
    let mut workers = HashMap::new();
    let mut live_workers = vec![];
    for live_worker in wm.worker_map.workers() {
        live_workers.push(live_worker.1.clone());
    }
    let mut lost_workers = vec![];
    for lost_worker in &wm.worker_map.lost_workers {
        lost_workers.push(lost_worker.1.clone());
    }
    workers.insert("live_workers".to_string(), live_workers);
    workers.insert("lost_workers".to_string(), lost_workers);
    Ok(Json(workers))
}

async fn auth_login_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<LoginRequest>,
) -> FsResult<Response> {
    let username = auth_username();
    let Some(password) = auth_password() else {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "success": false,
                "data": null,
                "error": { "code": "AUTH_NOT_CONFIGURED", "message": "web login password is not configured" }
            })),
        )
            .into_response());
    };

    if payload.username != username || !constant_time_eq(&payload.password, &password) {
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "success": false,
                "data": null,
                "error": { "code": "INVALID_CREDENTIALS", "message": "invalid username or password" }
            })),
        )
            .into_response());
    }

    let token = new_session_token();
    let expires_at = now_secs() + auth_session_ttl_secs();
    if let Ok(mut sessions) = instance.auth_sessions.lock() {
        sessions.insert(token.clone(), expires_at);
    }

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, session_cookie(&token))],
        Json(json!({
            "success": true,
            "data": { "username": payload.username, "expires_at": expires_at },
            "error": null
        })),
    )
        .into_response())
}

async fn auth_session_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    headers: HeaderMap,
) -> FsResult<Json<Value>> {
    let authenticated = is_authenticated(&instance, &headers);
    Ok(api_success(json!({
        "authenticated": authenticated,
        "username": if authenticated { Value::String(auth_username()) } else { Value::Null },
        "configured": auth_password().is_some()
    })))
}

async fn auth_logout_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    headers: HeaderMap,
) -> FsResult<Response> {
    if let Some(token) = cookie_value(&headers, AUTH_COOKIE_NAME) {
        if let Ok(mut sessions) = instance.auth_sessions.lock() {
            sessions.remove(&token);
        }
    }

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, expired_session_cookie())],
        Json(json!({ "success": true, "data": { "logged_out": true }, "error": null })),
    )
        .into_response())
}

async fn overview_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    let Json(data) = overview(Extension(instance)).await?;
    Ok(api_success(data))
}

async fn config_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    Ok(api_success(json!(instance.conf)))
}

async fn browse_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Value>> {
    let root_path = instance.fs.fs_dir.read().root_dir().name().to_string();
    let path = params.get("path").unwrap_or(&root_path);
    let cache_only = params
        .get("cache_only")
        .map(|value| value == "true" || value == "1")
        .unwrap_or(false);

    let mut items = match instance.unified_client(cache_only) {
        Some(client) => client.list_status(&FsPath::from_str(path)?).await?,
        None => instance.fs.list_status(path)?,
    };

    for item in &mut items {
        if item.path.contains("://") {
            item.storage_policy.state = StorageState::Ufs;
        }
    }

    Ok(api_success(json!({
        "items": items,
        "cache_only": cache_only,
        "page": 1,
        "page_size": items.len(),
        "total": items.len()
    })))
}

async fn block_locations_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Value>> {
    let Json(data) = block_locations(Extension(instance), Query(params)).await?;
    Ok(api_success(json!(data)))
}

async fn fs_stat_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Value>> {
    let fs = &instance.fs;
    let path = match params.get("path") {
        Some(path) => path,
        None => return err_box!("not found path"),
    };
    let status = fs.file_status(path)?;
    Ok(api_success(json!(status)))
}

async fn fs_mkdir_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<FsPathRequest>,
) -> FsResult<Json<Value>> {
    let create_parent = payload.create_parent.unwrap_or(true);
    match instance.unified_client(false) {
        Some(client) => {
            let created = client
                .mkdir(&FsPath::from_str(&payload.path)?, create_parent)
                .await?;
            Ok(api_success(json!({ "created": created })))
        }
        None => {
            let status = instance.fs.mkdir(payload.path, create_parent)?;
            Ok(api_success(json!(status)))
        }
    }
}

async fn fs_delete_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<FsPathRequest>,
) -> FsResult<Json<Value>> {
    let recursive = payload.recursive.unwrap_or(false);
    match instance.unified_client(false) {
        Some(client) => {
            client
                .delete(&FsPath::from_str(&payload.path)?, recursive)
                .await?;
            Ok(api_success(json!({ "deleted": true })))
        }
        None => {
            let deleted = instance.fs.delete(payload.path, recursive)?;
            Ok(api_success(json!({ "deleted": deleted })))
        }
    }
}

async fn fs_free_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<FsPathRequest>,
) -> FsResult<Json<Value>> {
    let result = instance
        .fs
        .free(&payload.path, payload.recursive.unwrap_or(false))?;

    Ok(api_success(json!({
        "inodes": result.inodes,
        "bytes": result.bytes
    })))
}

async fn fs_ufs_sync_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Value>> {
    let path = match params.get("path") {
        Some(path) => path,
        None => return err_box!("not found path"),
    };
    let fs_path = FsPath::from_str(path)?;
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };
    let Some((_ufs_path, mount)) = client.get_mount(&fs_path).await? else {
        return Ok(api_success(json!({
            "path": path,
            "mounted": false,
            "sync_supported": false,
            "state": "NotMounted",
            "done": true
        })));
    };

    if !mount.info.is_fs_mode() {
        return Ok(api_success(json!({
            "path": path,
            "mounted": true,
            "sync_supported": false,
            "write_type": format!("{:?}", mount.info.write_type),
            "state": "Unsupported",
            "done": true
        })));
    }

    let job_id = CommonUtils::create_job_id(fs_path.full_path());
    match with_job_timeout(instance.job_client()?.get_job_status(&job_id)).await {
        Ok(status) => {
            if !is_cv_to_ufs_sync(&status.source_path, &status.target_path) {
                return Ok(api_success(json!({
                    "path": path,
                    "job_id": status.job_id,
                    "id": status.job_id,
                    "mounted": true,
                    "sync_supported": true,
                    "state": "NotStarted",
                    "status": "NotStarted",
                    "message": "No Curvine to UFS sync job is running for this path",
                    "progress": 0.0,
                    "total_size": 0,
                    "loaded_size": 0,
                    "total_files": 0,
                    "completed_files": 0,
                    "failed_files": 0,
                    "running_files": 0,
                    "done": false
                })));
            }
            remember_load_job(&instance.load_jobs, status.job_id.clone());
            let mut payload = job_status_payload(status);
            if let Some(object) = payload.as_object_mut() {
                object.insert("path".to_string(), json!(path));
                object.insert("mounted".to_string(), json!(true));
                object.insert("sync_supported".to_string(), json!(true));
                object.insert("mount_path".to_string(), json!(mount.info.cv_path.clone()));
                object.insert("ufs_path".to_string(), json!(_ufs_path.full_path()));
            }
            cache_load_job(&instance.load_job_cache, &job_id, payload.clone());
            Ok(api_success(payload))
        }
        Err(err) if matches!(err.kind(), ErrorKind::JobNotFound) => Ok(api_success(json!({
            "path": path,
            "job_id": job_id,
            "id": job_id,
            "mounted": true,
            "sync_supported": true,
            "state": "NotStarted",
            "status": "NotStarted",
            "message": "Waiting for UFS sync job to be created",
            "progress": 0.0,
            "total_size": 0,
            "loaded_size": 0,
            "total_files": 0,
            "completed_files": 0,
            "failed_files": 0,
            "running_files": 0,
            "done": false
        }))),
        Err(err) => Err(err),
    }
}

async fn fs_ufs_sync_jobs_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(query): Query<UfsSyncJobsQuery>,
) -> FsResult<Json<Value>> {
    let path_prefix = query
        .path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("/");
    let fs_path = FsPath::from_str(path_prefix)?;
    let normalized_prefix = fs_path.full_path().to_string();
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let include_finished = query.include_finished.unwrap_or(true);
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };
    let statuses = match with_job_timeout(instance.job_client()?.list_job_statuses_with_options(
        JobListOptions {
            path_prefix: Some(normalized_prefix.clone()),
            limit,
            include_finished,
            state: query.state.as_deref().and_then(parse_job_state),
            include_tasks: query.include_tasks.unwrap_or(false),
            failed_only: query.failed_only.unwrap_or(false),
            offset: query.offset.unwrap_or(0),
            ..Default::default()
        },
    ))
    .await
    {
        Ok(statuses) => statuses,
        Err(err) if err.to_string().contains("Unsupported operation") => Vec::new(),
        Err(err) => return Err(err),
    };

    let mut items = Vec::new();
    for status in statuses {
        if !is_cv_to_ufs_sync(&status.source_path, &status.target_path) {
            continue;
        }
        let source_path = status.source_path.clone();
        let Ok(source_fs_path) = FsPath::from_str(&source_path) else {
            continue;
        };
        let Ok(Some((_ufs_path, mount))) = client.get_mount(&source_fs_path).await else {
            continue;
        };
        if !mount.info.is_fs_mode() {
            continue;
        }
        let mut payload = job_status_payload(status);
        if let Some(object) = payload.as_object_mut() {
            object.insert("path".to_string(), json!(source_path));
            object.insert("mounted".to_string(), json!(true));
            object.insert("sync_supported".to_string(), json!(true));
            object.insert("mount_path".to_string(), json!(mount.info.cv_path.clone()));
            object.insert("ufs_path".to_string(), json!(_ufs_path.full_path()));
        }
        if let Some(job_id) = payload.get("job_id").and_then(|value| value.as_str()) {
            remember_load_job(&instance.load_jobs, job_id.to_string());
            cache_load_job(&instance.load_job_cache, job_id, payload.clone());
        }
        items.push(payload);
    }
    if items.is_empty() {
        let cached = instance
            .load_job_cache
            .lock()
            .map(|cache| cache.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for payload in cached {
            if !is_cv_to_ufs_sync_payload(&payload) {
                continue;
            }
            let Some(path) = payload.get("path").and_then(|value| value.as_str()) else {
                continue;
            };
            let path_matches = normalized_prefix == "/"
                || path == normalized_prefix
                || path.starts_with(&format!("{}/", normalized_prefix.trim_end_matches('/')));
            if !path_matches {
                continue;
            }
            let Ok(source_fs_path) = FsPath::from_str(path) else {
                continue;
            };
            let Ok(Some((_ufs_path, mount))) = client.get_mount(&source_fs_path).await else {
                continue;
            };
            if mount.info.is_fs_mode() {
                items.push(payload);
            }
        }
    }

    let total = items.len();
    Ok(api_success(json!({
        "items": items,
        "path": normalized_prefix,
        "total": total
    })))
}

async fn fs_upload_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> FsResult<Json<Value>> {
    let path = match params.get("path") {
        Some(path) => path,
        None => return err_box!("not found path"),
    };
    let overwrite = params
        .get("overwrite")
        .map(|value| value == "true" || value == "1")
        .unwrap_or(true);
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };

    let mut writer = client.create(&FsPath::from_str(path)?, overwrite).await?;
    writer.write(&body).await?;
    writer.complete().await?;

    Ok(api_success(json!({
        "path": path,
        "bytes": body.len()
    })))
}

async fn fs_download_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Response> {
    let path = match params.get("path") {
        Some(path) => path,
        None => return err_box!("not found path"),
    };
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };

    let fs_path = FsPath::from_str(path)?;
    let status = client.get_status(&fs_path).await?;
    if status.is_dir {
        return err_box!("cannot download directory: {}", path);
    }

    let mut reader = client.open(&fs_path).await?;
    let total_size = reader.len();
    let chunk_size = client.conf().client.read_chunk_size;
    let mut remaining = total_size;
    let mut content = BytesMut::with_capacity(total_size.max(0) as usize);
    while remaining > 0 {
        let to_read = std::cmp::min(remaining as usize, chunk_size);
        let start = content.len();
        content.resize(start + to_read, 0);
        reader
            .read_full(&mut content[start..start + to_read])
            .await?;
        remaining -= to_read as i64;
    }
    reader.complete().await?;

    let filename = fs_path.name();
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename.replace('"', "")),
            ),
        ],
        Body::from(content.freeze()),
    )
        .into_response())
}

async fn workers_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    let fs = &instance.fs;
    let wm = fs.worker_manager.read();
    let mut live_workers = vec![];
    let mut blacklist_workers = vec![];
    let mut decommission_workers = vec![];

    for (_, worker) in wm.worker_map.workers() {
        match worker.status {
            WorkerStatus::Blacklist => blacklist_workers.push(worker.clone()),
            WorkerStatus::Decommission => decommission_workers.push(worker.clone()),
            _ => live_workers.push(worker.clone()),
        }
    }

    let lost_workers: Vec<WorkerInfo> = wm
        .worker_map
        .lost_workers()
        .iter()
        .map(|(_, worker)| worker.clone())
        .collect();
    let total = live_workers.len()
        + blacklist_workers.len()
        + decommission_workers.len()
        + lost_workers.len();

    Ok(api_success(json!({
        "live_workers": live_workers,
        "blacklist_workers": blacklist_workers,
        "decommission_workers": decommission_workers,
        "lost_workers": lost_workers,
        "total": total
    })))
}

async fn worker_detail_v1(
    Path(worker): Path<String>,
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    let fs = &instance.fs;
    let wm = fs.worker_manager.read();

    for (_, item) in wm.worker_map.workers() {
        if worker_matches(item, &worker) {
            return Ok(api_success(worker_payload(
                item.clone(),
                &format!("{:?}", item.status),
            )));
        }
    }

    for (_, item) in wm.worker_map.lost_workers() {
        if worker_matches(item, &worker) {
            return Ok(api_success(worker_payload(item.clone(), "Lost")));
        }
    }

    err_box!("worker not found: {}", worker)
}

async fn worker_action_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<WorkerActionRequest>,
) -> FsResult<Json<Value>> {
    let target_status = match payload.action.trim().to_lowercase().as_str() {
        "blacklist" | "block" => WorkerStatus::Blacklist,
        "allow" | "unblacklist" | "remove_blacklist" => WorkerStatus::Live,
        "decommission" | "retire" => WorkerStatus::Decommission,
        "recommission" | "remove_decommission" => WorkerStatus::Live,
        action => return err_box!("unsupported worker action: {}", action),
    };

    let fs = &instance.fs;
    let mut wm = fs.worker_manager.write();
    for (_, worker) in wm.worker_map.workers.iter_mut() {
        if worker_matches(worker, &payload.worker) {
            worker.status = target_status;
            return Ok(api_success(worker_payload(
                worker.clone(),
                &format!("{:?}", worker.status),
            )));
        }
    }

    err_box!("worker not found or not manageable: {}", payload.worker)
}

fn parse_mount_options(payload: &MountRequest) -> FsResult<MountOptions> {
    let write_type = WriteType::try_from(payload.write_type.as_str())?;
    let ttl_ms = DurationUnit::from_str(payload.ttl.as_str())?.as_millis() as i64;
    let ttl_action = if matches!(write_type, WriteType::FsMode) {
        TtlAction::Free
    } else {
        TtlAction::Delete
    };

    let mut opts = MountOptions::builder()
        .update(payload.update)
        .set_properties(payload.properties.clone())
        .write_type(write_type)
        .read_verify_ufs(payload.read_verify_ufs)
        .ttl_ms(ttl_ms)
        .ttl_action(ttl_action);

    if let Some(replicas) = payload.replicas {
        opts = opts.replicas(replicas);
    }
    if let Some(block_size) = payload.block_size.as_ref().filter(|v| !v.trim().is_empty()) {
        opts = opts.block_size(ByteUnit::from_str(block_size.as_str())?.as_byte() as i64);
    }
    if let Some(storage_type) = payload
        .storage_type
        .as_ref()
        .filter(|v| !v.trim().is_empty())
    {
        opts = opts.storage_type(StorageType::try_from(storage_type.as_str())?);
    }
    if let Some(provider) = payload.provider.as_ref().filter(|v| !v.trim().is_empty()) {
        opts = opts.provider(Provider::try_from(provider.as_str())?);
    }

    Ok(opts.build())
}

async fn mounts_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    let items = match instance.unified_client(false) {
        Some(client) => client.get_mount_table().await?,
        None => vec![],
    };

    Ok(api_success(json!({
        "items": items,
        "page": 1,
        "page_size": items.len(),
        "total": items.len()
    })))
}

async fn mount_create_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<MountRequest>,
) -> FsResult<Json<Value>> {
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };

    if payload.cv_path.trim().is_empty() {
        return err_box!("curvine path is required");
    }
    if payload.ufs_path.trim().is_empty() {
        return err_box!("ufs path is required");
    }

    let normalized_ufs_path = normalize_ufs_path(&payload.ufs_path);
    let ufs_path = FsPath::from_str(&normalized_ufs_path)?;
    let cv_path = FsPath::from_str(payload.cv_path.trim())?;
    let opts = parse_mount_options(&payload)?;

    let ufs = UfsFileSystem::new(&ufs_path, opts.add_properties.clone(), opts.provider)?;
    ufs.list_status(&ufs_path).await?;

    client.mount(&ufs_path, &cv_path, opts).await?;
    Ok(api_success(json!({
        "mounted": true,
        "cv_path": payload.cv_path,
        "ufs_path": normalized_ufs_path
    })))
}

async fn mount_delete_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<MountPathRequest>,
) -> FsResult<Json<Value>> {
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };
    client.umount(&FsPath::from_str(&payload.cv_path)?).await?;
    Ok(api_success(
        json!({ "deleted": true, "cv_path": payload.cv_path }),
    ))
}

async fn mount_validate_v1(Json(payload): Json<MountValidateRequest>) -> FsResult<Json<Value>> {
    if payload.ufs_path.trim().is_empty() {
        return err_box!("ufs path is required");
    }
    let normalized_ufs_path = normalize_ufs_path(&payload.ufs_path);
    let ufs_path = FsPath::from_str(&normalized_ufs_path)?;
    let provider = match payload.provider.as_ref().filter(|v| !v.trim().is_empty()) {
        Some(provider) => Some(Provider::try_from(provider.as_str())?),
        None => None,
    };
    let ufs = UfsFileSystem::new(&ufs_path, payload.properties.clone(), provider)?;
    let items = ufs.list_status(&ufs_path).await?;
    Ok(api_success(json!({
        "valid": true,
        "entries": items.len(),
        "ufs_path": normalized_ufs_path
    })))
}

async fn mount_resync_start_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<MountResyncRequest>,
) -> FsResult<Json<Value>> {
    let client = match instance.unified_client(false) {
        Some(client) => client,
        None => return err_box!("unified fs client is unavailable"),
    };

    let cv_path = FsPath::from_str(&payload.cv_path)?;
    if !cv_path.is_cv() {
        return err_box!("resync requires a curvine path, got: {}", payload.cv_path);
    }

    let mount = match client.get_mount_info(&cv_path).await? {
        Some(value) => value,
        None => return err_box!("mount info not found for {}", payload.cv_path),
    };
    if !matches!(mount.write_type, WriteType::FsMode) {
        return err_box!(
            "resync is only allowed for fs_mode mount: {}",
            mount.cv_path
        );
    }

    let task_id = new_task_id();
    let now = LocalTime::now_datetime();
    let task = ResyncTaskState {
        id: task_id.clone(),
        cv_path: payload.cv_path.clone(),
        mount_path: mount.cv_path.clone(),
        write_type: "fs_mode".to_string(),
        status: "queued".to_string(),
        scanned: 0,
        skipped: 0,
        recreated: 0,
        failed: 0,
        pending_dirs: 0,
        message: "queued".to_string(),
        started_at: now.clone(),
        updated_at: now,
        done: false,
        dry_run: payload.dry_run,
    };

    if let Ok(mut tasks) = instance.resync_tasks.lock() {
        tasks.insert(task_id.clone(), task.clone());
    }

    let tasks = instance.resync_tasks.clone();
    let cv_path_text = payload.cv_path.clone();
    tokio::spawn(async move {
        run_mount_resync_task(client, tasks, task_id, cv_path_text, payload.dry_run).await;
    });

    Ok(api_success(json!(task)))
}

async fn mount_resync_status_v1(
    Path(task_id): Path<String>,
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    let task = match instance.resync_tasks.lock() {
        Ok(tasks) => tasks.get(&task_id).cloned(),
        Err(_) => None,
    };

    match task {
        Some(task) => Ok(api_success(json!(task))),
        None => err_box!("resync task not found: {}", task_id),
    }
}

async fn run_mount_resync_task(
    fs: UnifiedFileSystem,
    tasks: Arc<Mutex<HashMap<String, ResyncTaskState>>>,
    task_id: String,
    cv_path_text: String,
    dry_run: bool,
) {
    update_resync_task(&tasks, &task_id, |task| {
        task.status = "running".to_string();
        task.message = "starting".to_string();
    });

    let result: FsResult<()> = async {
        let cv_path = FsPath::from_str(&cv_path_text)?;
        let client = fs.fs_client();
        let mount = match client.get_mount_info(&cv_path).await? {
            Some(value) => value,
            None => return err_box!("mount info not found for {}", cv_path),
        };

        if !matches!(mount.write_type, WriteType::FsMode) {
            return err_box!(
                "resync is only allowed for fs_mode mount: {}",
                mount.cv_path
            );
        }

        let ufs_root = FsPath::from_str(&mount.ufs_path)?;
        let ufs = UfsFileSystem::new(&ufs_root, mount.properties.clone(), mount.provider)?;
        let mut queue = VecDeque::from([ufs_root.clone()]);
        update_resync_task(&tasks, &task_id, |task| {
            task.mount_path = mount.cv_path.clone();
            task.pending_dirs = queue.len();
            task.message = format!("scanning {}", ufs_root);
        });

        while let Some(ufs_dir) = queue.pop_front() {
            let ufs_entries = match ufs.list_status(&ufs_dir).await {
                Ok(value) => value,
                Err(err) => {
                    update_resync_task(&tasks, &task_id, |task| {
                        task.failed += 1;
                        task.pending_dirs = queue.len();
                        task.message = format!("failed to list ufs dir {}: {}", ufs_dir, err);
                    });
                    continue;
                }
            };

            let cv_dir = mount.get_cv_path(&ufs_dir)?;
            if !dry_run {
                if let Err(err) = fs.cv().mkdir(&cv_dir, true).await {
                    update_resync_task(&tasks, &task_id, |task| {
                        task.failed += 1;
                        task.pending_dirs = queue.len();
                        task.message = format!("failed to create cv dir {}: {}", cv_dir, err);
                    });
                    continue;
                }
            }

            let cv_entries = match fs.cv().list_status(&cv_dir).await {
                Ok(value) => value,
                Err(err) if is_cv_dir_missing(&err) => vec![],
                Err(err) => {
                    update_resync_task(&tasks, &task_id, |task| {
                        task.failed += 1;
                        task.pending_dirs = queue.len();
                        task.message = format!("failed to list cv dir {}: {}", cv_dir, err);
                    });
                    continue;
                }
            };

            let mut cv_map = HashMap::new();
            for entry in cv_entries {
                cv_map.insert(entry.path.to_string(), entry);
            }

            for ufs_entry in ufs_entries {
                let ufs_path = FsPath::from_str(ufs_entry.path)?;
                if ufs_entry.is_dir {
                    queue.push_back(ufs_path);
                    update_resync_task(&tasks, &task_id, |task| {
                        task.pending_dirs = queue.len();
                        task.message = format!("queued {} directories", queue.len());
                    });
                    continue;
                }

                let ufs_mtime = ufs_entry.mtime;
                let ufs_len = ufs_entry.len;
                let cv_path = mount.get_cv_path(&ufs_path)?;
                let cv_key = cv_path.full_path().to_string();

                let mut recreate = true;
                if let Some(cv_status) = cv_map.get(&cv_key) {
                    let cv_ufs_mtime = cv_status.storage_policy.ufs_mtime;
                    if cv_ufs_mtime == 0 || cv_ufs_mtime == ufs_mtime {
                        recreate = false;
                    } else if !dry_run {
                        if let Err(err) = client.delete(&cv_path, false).await {
                            update_resync_task(&tasks, &task_id, |task| {
                                task.scanned += 1;
                                task.failed += 1;
                                task.pending_dirs = queue.len();
                                task.message = format!("failed to delete {}: {}", cv_path, err);
                            });
                            continue;
                        }
                    }
                }

                if !recreate {
                    update_resync_task(&tasks, &task_id, |task| {
                        task.scanned += 1;
                        task.skipped += 1;
                        task.pending_dirs = queue.len();
                        task.message = format!("skipped {}", cv_path);
                    });
                    continue;
                }

                if !dry_run {
                    let create_opts = mount.get_sync_opts(&fs.conf().client, ufs_mtime, ufs_len);
                    if let Err(err) = client.create_with_opts(&cv_path, create_opts, false).await {
                        update_resync_task(&tasks, &task_id, |task| {
                            task.scanned += 1;
                            task.failed += 1;
                            task.pending_dirs = queue.len();
                            task.message = format!("failed to create {}: {}", cv_path, err);
                        });
                        continue;
                    }
                }

                update_resync_task(&tasks, &task_id, |task| {
                    task.scanned += 1;
                    task.recreated += 1;
                    task.pending_dirs = queue.len();
                    task.message = format!("synced {}", cv_path);
                });
            }
        }

        Ok(())
    }
    .await;

    match result {
        Ok(()) => update_resync_task(&tasks, &task_id, |task| {
            task.status = "completed".to_string();
            task.pending_dirs = 0;
            task.done = true;
            task.message = "completed".to_string();
        }),
        Err(err) => update_resync_task(&tasks, &task_id, |task| {
            task.status = "failed".to_string();
            task.done = true;
            task.message = err.to_string();
        }),
    }
}

async fn jobs_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(query): Query<JobListQuery>,
) -> FsResult<Json<Value>> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let state_filter = query.state.unwrap_or_default().to_lowercase();
    let job_ids: Vec<String> = instance
        .load_jobs
        .lock()
        .map(|jobs| jobs.iter().cloned().collect())
        .unwrap_or_default();
    let cached = instance
        .load_job_cache
        .lock()
        .map(|cache| cache.clone())
        .unwrap_or_default();
    let mut items = Vec::new();

    for job_id in job_ids {
        let item = cached.get(&job_id).cloned().unwrap_or_else(|| {
            json!({
                "job_id": job_id,
                "id": job_id,
                "state": "Unknown",
                "status": "Unknown",
                "message": "status has not been queried yet",
                "progress": 0.0,
                "done": false
            })
        });
        let state_name = item
            .get("state")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_lowercase();
        if state_filter.is_empty()
            || state_filter == "all"
            || state_filter == state_name
            || (state_filter == "running" && matches!(state_name.as_str(), "loading" | "pending"))
        {
            items.push(item);
        }
    }

    let total = items.len();
    let start = (page - 1) * page_size;
    let paged = items
        .into_iter()
        .skip(start)
        .take(page_size)
        .collect::<Vec<_>>();
    Ok(api_success(json!({
        "items": paged,
        "page": page,
        "page_size": page_size,
        "total": total
    })))
}

async fn submit_load_v1(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Json(payload): Json<SubmitLoadRequest>,
) -> FsResult<Json<Value>> {
    let source_path = payload.path.trim();
    if source_path.is_empty() {
        return err_box!("path is required");
    }

    let mut builder = LoadJobCommand::builder(source_path);
    if let Some(target_path) = payload
        .target_path
        .as_ref()
        .filter(|v| !v.trim().is_empty())
    {
        builder = builder.target_path(target_path.trim());
    }
    if let Some(replicas) = payload.replicas {
        builder = builder.replicas(replicas);
    }
    if let Some(block_size) = payload.block_size.as_ref().filter(|v| !v.trim().is_empty()) {
        builder = builder.block_size(ByteUnit::from_str(block_size.as_str())?.as_byte() as i64);
    }
    if let Some(storage_type) = payload
        .storage_type
        .as_ref()
        .filter(|v| !v.trim().is_empty())
    {
        builder = builder.storage_type(StorageType::try_from(storage_type.as_str())?);
    }
    if let Some(ttl) = payload.ttl.as_ref().filter(|v| !v.trim().is_empty()) {
        builder = builder.ttl_ms(DurationUnit::from_str(ttl.as_str())?.as_millis() as i64);
    }
    if let Some(ttl_action) = payload.ttl_action.as_ref().filter(|v| !v.trim().is_empty()) {
        builder = builder.ttl_action(TtlAction::try_from(ttl_action.as_str())?);
    }
    if let Some(overwrite) = payload.overwrite {
        builder = builder.overwrite(overwrite);
    }

    if payload.recursive {
        log::info!("web load request recursive=true; current load command expands directories on the job runner side when supported");
    }
    if !payload.configs.is_empty() {
        log::info!("web load request included {} config entries; mounted UFS configuration is used by current backend", payload.configs.len());
    }

    let result = with_job_timeout(instance.job_client()?.submit_load_job(builder.build())).await?;
    remember_load_job(&instance.load_jobs, result.job_id.clone());
    let payload = json!({
        "job_id": result.job_id,
        "id": result.job_id,
        "path": source_path,
        "source_path": source_path,
        "target_path": result.target_path,
        "state": job_state_name(result.state),
        "status": job_state_name(result.state),
        "progress": if result.state == JobTaskState::Completed { 100.0 } else { 0.0 },
        "done": result.state.is_finish()
    });
    cache_load_job(
        &instance.load_job_cache,
        result.job_id.as_str(),
        payload.clone(),
    );

    Ok(api_success(payload))
}

async fn job_status_v1(
    Path(job_id): Path<String>,
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    if job_id.trim().is_empty() {
        return err_box!("job_id is required");
    }
    let status = with_job_timeout(instance.job_client()?.get_job_status(&job_id)).await?;
    remember_load_job(&instance.load_jobs, status.job_id.clone());
    let payload = job_status_payload(status);
    cache_load_job(&instance.load_job_cache, &job_id, payload.clone());
    Ok(api_success(payload))
}

async fn job_cancel_v1(
    Path(job_id): Path<String>,
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Value>> {
    if job_id.trim().is_empty() {
        return err_box!("job_id is required");
    }

    if let Some(cached) = instance
        .load_job_cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&job_id).cloned())
    {
        if cached
            .get("done")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Ok(api_success(cached));
        }
    }

    with_job_timeout(instance.job_client()?.cancel_job(&job_id)).await?;
    remember_load_job(&instance.load_jobs, job_id.clone());
    let payload = json!({
        "job_id": job_id,
        "id": job_id,
        "state": "Canceled",
        "status": "Canceled",
        "cancelled": true,
        "done": true
    });
    cache_load_job(&instance.load_job_cache, &job_id, payload.clone());
    Ok(api_success(payload))
}

async fn add_dcm(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Vec<String>>> {
    let fs = &instance.fs;
    let mut wm = fs.worker_manager.write();
    match params.get("workers") {
        None => err_box!("not params workers"),
        Some(v) => {
            let list = v.split(",").map(|x| x.to_string()).collect();
            let res = wm.add_dcm(list);
            Ok(Json(res))
        }
    }
}

async fn get_dcm(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Json<Vec<String>>> {
    let fs = &instance.fs;
    let wm = fs.worker_manager.read();
    Ok(Json(wm.get_dcm()))
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("text/html"))
        .unwrap_or(false)
}

fn webui_index_response() -> FsResult<Response> {
    let body = std::fs::read_to_string("webui/index.html")?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response())
}

async fn spa_index() -> FsResult<Response> {
    webui_index_response()
}

async fn remove_dcm(
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
    Query(params): Query<HashMap<String, String>>,
) -> FsResult<Json<Vec<String>>> {
    let fs = &instance.fs;
    let mut wm = fs.worker_manager.write();
    match params.get("workers") {
        None => err_box!("not params workers"),
        Some(v) => {
            let list = v.split(",").map(|x| x.to_string()).collect();
            let res = wm.remove_dcm(list);
            Ok(Json(res))
        }
    }
}

async fn workers1(
    headers: HeaderMap,
    Extension(instance): Extension<Arc<MasterRouterHandler>>,
) -> FsResult<Response> {
    if wants_html(&headers) {
        return webui_index_response();
    }

    let fs = &instance.fs;
    let wm = fs.worker_manager.read();
    Ok(Json(wm.worker_list()).into_response())
}

impl RouterHandler for MasterRouterHandler {
    fn router(&self) -> Router {
        let instance = Arc::new(self.clone());
        let conf = self.conf.clone();
        Router::new()
            .route("/metrics", get(metrics))
            .route("/report", get(report))
            .route("/api/overview", get(overview))
            .route("/api/config", get(|| async { Json(conf) }))
            .route("/api/browse", get(browse))
            .route("/api/block_locations", get(block_locations))
            .route("/api/workers", get(workers))
            .route("/api/v1/auth/login", post(auth_login_v1))
            .route("/api/v1/auth/session", get(auth_session_v1))
            .route("/api/v1/auth/logout", post(auth_logout_v1))
            .route("/api/v1/overview", get(overview_v1))
            .route("/api/v1/config", get(config_v1))
            .route("/api/v1/fs/list", get(browse_v1))
            .route("/api/v1/fs/blocks", get(block_locations_v1))
            .route("/api/v1/fs/stat", get(fs_stat_v1))
            .route("/api/v1/fs/ufs-sync/jobs", get(fs_ufs_sync_jobs_v1))
            .route("/api/v1/fs/ufs-sync", get(fs_ufs_sync_v1))
            .route("/api/v1/fs/mkdir", post(fs_mkdir_v1))
            .route("/api/v1/fs/delete", post(fs_delete_v1))
            .route("/api/v1/fs/free", post(fs_free_v1))
            .route("/api/v1/fs/upload", post(fs_upload_v1))
            .route("/api/v1/fs/download", get(fs_download_v1))
            .route("/api/v1/workers", get(workers_v1))
            .route("/api/v1/workers/detail/:worker", get(worker_detail_v1))
            .route("/api/v1/workers/action", post(worker_action_v1))
            .route("/api/v1/mounts", get(mounts_v1).post(mount_create_v1))
            .route("/api/v1/mounts/delete", post(mount_delete_v1))
            .route("/api/v1/mounts/validate", post(mount_validate_v1))
            .route("/api/v1/mounts/resync", post(mount_resync_start_v1))
            .route(
                "/api/v1/mounts/resync/:task_id",
                get(mount_resync_status_v1),
            )
            .route("/api/v1/jobs", get(jobs_v1))
            .route("/api/v1/jobs/load", get(jobs_v1).post(submit_load_v1))
            .route(
                "/api/v1/jobs/:job_id",
                get(job_status_v1).delete(job_cancel_v1),
            )
            .route("/add-dcm", get(add_dcm))
            .route("/get-dcm", get(get_dcm))
            .route("/remove-dcm", get(remove_dcm))
            .route("/login", get(spa_index))
            .route("/overview", get(spa_index))
            .route("/browse", get(spa_index))
            .route("/mounts", get(spa_index))
            .route("/jobs", get(spa_index))
            .route("/config", get(spa_index))
            .route("/blocks", get(spa_index))
            .route("/preview", get(spa_index))
            .route("/workers", get(workers1))
            .route("/workers/:id", get(spa_index))
            .route_layer(middleware::from_fn(require_auth))
            .layer(Extension(instance))
    }
}

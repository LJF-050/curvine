/*
 * Copyright 2025 OPPO.
 *
 * Licensed under the Apache License, Version 2.0.
 */

import request from '@/utils/request'

function unwrap(response) {
  const payload = response && response.data !== undefined ? response.data : response
  if (typeof payload === 'string') {
    const trimmed = payload.trim().toLowerCase()
    if (trimmed.startsWith('<!doctype') || trimmed.startsWith('<html')) {
      throw new Error('Expected JSON API response, got HTML fallback')
    }
  }
  if (payload && payload.success === true) {
    return payload.data
  }
  if (payload && payload.success === false) {
    throw new Error(payload.error?.message || 'API request failed')
  }
  return payload
}

// All data endpoints are served by the authenticated /v1 API. The legacy
// non-versioned routes (/api/overview, /api/browse, ...) are unauthenticated,
// so we must NOT silently fall back to them: a rejected/failed v1 request must
// surface the error instead of leaking data through an anonymous route.
async function getV1(url, options = {}) {
  return unwrap(await request({ url, method: 'get', ...options }))
}

async function postV1(url, data) {
  const result = unwrap(await request({ url, method: 'post', data }))
  return result
}


export function login(data) {
  return postV1('/v1/auth/login', data)
}

export function fetchAuthSession() {
  return getV1('/v1/auth/session')
}

export function logout() {
  return postV1('/v1/auth/logout', {})
}

export function fetchOverviewData() {
  return getV1('/v1/overview')
}
export function fetchMasterHa() {
  return getV1('/v1/master/ha')
}

export async function requestMasterFailover(targetMaster) {
  return unwrap(await request({ url: '/v1/master/failover', method: 'post', data: { target_master: targetMaster }, timeout: 180000 }))
}

export function fetchConfigData() {
  return getV1('/v1/config')
}

export function fetchBrowseData(query) {
  return getV1('/v1/fs/list', { params: query })
}

export function fetchDirectoryCacheSummary(path) {
  return getV1('/v1/fs/cache-summary', { params: { path } })
}

export function fetchWorkersData() {
  return getV1('/v1/workers')
}

export function fetchWorkerDetail(worker) {
  return getV1(`/v1/workers/detail/${encodeURIComponent(worker)}`)
}

export function updateWorkerAction(worker, action) {
  return postV1('/v1/workers/action', { worker, action })
}

export function fetchBlocksData(query) {
  return getV1('/v1/fs/blocks', { params: query })
}

export function fetchUfsSyncStatus(path) {
  return getV1('/v1/fs/ufs-sync', { params: { path } })
}

export function fetchUfsSyncJobs(path, limit = 100, includeFinished = true, options: { includeTasks?: boolean; failedOnly?: boolean; state?: string } = {}) {
  return getV1('/v1/fs/ufs-sync/jobs', {
    params: {
      path,
      limit,
      include_finished: includeFinished,
      include_tasks: options.includeTasks || false,
      failed_only: options.failedOnly || false,
      state: options.state || undefined
    }
  })
}

export function fetchMountsData() {
  return getV1('/v1/mounts')
}

export function saveMount(data) {
  return postV1('/v1/mounts', data)
}

export function deleteMount(cvPath) {
  return postV1('/v1/mounts/delete', { cv_path: cvPath })
}

export function validateMount(data) {
  return postV1('/v1/mounts/validate', data)
}

export function startMountResync(cvPath, dryRun = false) {
  return postV1('/v1/mounts/resync', { cv_path: cvPath, dry_run: dryRun })
}

export function fetchMountResync(taskId) {
  return getV1(`/v1/mounts/resync/${encodeURIComponent(taskId)}`)
}

export function fetchJobsData(params = {}) {
  return getV1('/v1/jobs/load', { params })
}

export function submitLoadJob(data) {
  return postV1('/v1/jobs/load', data)
}

export function fetchJobStatus(jobId) {
  return getV1(`/v1/jobs/${encodeURIComponent(jobId)}`)
}

export async function cancelLoadJob(jobId) {
  return unwrap(await request({ url: `/v1/jobs/${encodeURIComponent(jobId)}`, method: 'delete' }))
}


export function createDirectory(path, createParent = true) {
  return postV1('/v1/fs/mkdir', { path, create_parent: createParent })
}

export function deletePath(path, recursive = false) {
  return postV1('/v1/fs/delete', { path, recursive })
}

export function freePath(path, recursive = false) {
  return postV1('/v1/fs/free', { path, recursive })
}

export async function uploadFile(path, file, overwrite = true, onUploadProgress = null, timeout = 300000, syncUfs = true) {
  const result = unwrap(await request({
    url: `/v1/fs/upload?path=${encodeURIComponent(path)}&overwrite=${overwrite}&sync_ufs=${syncUfs}`,
    method: 'post',
    data: file,
    timeout,
    onUploadProgress: onUploadProgress || undefined
  }))
  return result
}

export function downloadFile(path) {
  return request({
    url: '/v1/fs/download',
    method: 'get',
    params: { path },
    responseType: 'blob'
  })
}

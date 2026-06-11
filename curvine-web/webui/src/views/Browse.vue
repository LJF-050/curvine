<template>
  <div class="admin-page" v-loading="loading">
    <section class="admin-panel table-panel">
      <div class="panel-heading table-heading"><div><h2>File Browser</h2><p>Browse Curvine namespace and inspect file blocks</p></div><div class="segmented"><button class="active">List</button><button @click="goBlocks">Blocks</button></div></div>
      <div class="path-toolbar"><button class="admin-button ghost compact" @click="setRoot">Root</button><button class="admin-button ghost compact" :disabled="!canGoUp" title="Open parent folder" @click="goUp">Up</button><input v-model="browsePath" class="admin-input path-input" @keyup.enter="navigate" placeholder="/path"><button class="admin-button primary compact" @click="navigate">Go</button><label class="cache-toggle"><input type="checkbox" v-model="cacheOnly" @change="fetchData"> Cache only</label><button class="admin-button ghost compact" @click="mkdirDialog">Mkdir</button><button class="admin-button ghost compact" @click="uploadFileDialog">Upload File</button><button class="admin-button ghost compact" @click="uploadFolderDialog">Upload Folder</button><button class="admin-button ghost compact" :disabled="!canFreeCurrentPath" :title="currentFreeTitle" @click="freeCurrentPath">Free</button><input ref="fileInput" class="hidden-file-input" type="file" @change="onFilePicked"><input ref="folderInput" class="hidden-file-input" type="file" webkitdirectory directory multiple @change="onFolderPicked"><span v-if="cacheOnly" class="cache-mode-note">Cache-only: CV-backed files shown, UFS-only files hidden</span></div>
      <div v-if="uploadNotice" class="upload-notice">{{ uploadNotice }}</div>
      <div v-if="uploadBatchList.length" class="sync-panel">
        <div class="sync-panel-title">
          <span>Upload Progress</span>
          <div class="sync-title-actions">
            <button v-if="hasCompletedUploadBatches" class="link-button" @click="clearCompletedUploadBatches">Clear completed</button>
            <button class="link-button" @click="clearUploadBatches">Clear</button>
          </div>
        </div>
        <div v-for="batch in uploadBatchList" :key="batch.id" class="sync-job batch-sync">
          <strong :title="batch.label">{{ batch.label }}</strong>
          <span :class="['sync-status', syncStatusClass(batch)]">{{ batch.state }}</span>
          <span>{{ batch.uploadedFiles }} / {{ batch.totalFiles }} uploaded</span>
          <span>{{ formatBytes(batch.uploadedSize) }}</span>
          <div class="sync-progress-cell"><div class="meter sync-meter"><span :style="{ width: batch.progress + '%' }"></span></div><strong>{{ batch.progress }}%</strong></div>
          <div v-if="batch.message" class="sync-message">{{ batch.message }}</div>
        </div>
      </div>
      <div v-if="ufsSyncJobCount" class="sync-panel ufs-sync-panel" :class="{ 'is-collapsed': !ufsSyncPanelExpanded }">
        <div class="sync-panel-header">
          <button
            type="button"
            class="sync-collapse-btn"
            :aria-expanded="ufsSyncPanelExpanded"
            :aria-label="ufsSyncPanelExpanded ? 'Collapse UFS sync panel' : 'Expand UFS sync panel'"
            :title="ufsSyncPanelExpanded ? 'Collapse' : 'Expand'"
            @click="toggleUfsSyncPanel"
          >
            <svg class="sync-collapse-icon" :class="{ expanded: ufsSyncPanelExpanded }" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          </button>
          <div class="sync-panel-heading-main" role="button" tabindex="0" @click="toggleUfsSyncPanel" @keydown.enter.prevent="toggleUfsSyncPanel" @keydown.space.prevent="toggleUfsSyncPanel">
            <span class="sync-panel-toggle-label">FS mode UFS Sync Progress</span>
            <span class="sync-panel-summary">{{ ufsSyncSummaryText }}</span>
          </div>
          <div class="sync-title-actions">
            <button class="link-button" @click.stop="$router.push('/jobs')">View details in Sync Jobs</button>
            <button v-if="hasCompletedUfsSyncJobs" class="link-button" @click.stop="clearCompletedUfsSyncJobs">Clear completed</button>
          </div>
        </div>
        <div v-show="ufsSyncPanelExpanded" class="sync-panel-body">
        <div v-for="batch in ufsSyncBatchList" :key="batch.id" class="sync-job batch-sync">
          <div class="sync-target" :title="batch.title || batch.label">
            <strong>{{ batch.label }}</strong>
            <small v-if="batch.subtitle">{{ batch.subtitle }}</small>
          </div>
          <span :class="['sync-status', syncStatusClass(batch)]">{{ batch.state }}</span>
          <span>{{ batch.syncedFiles }} / {{ batch.totalFiles }} UFS synced</span>
          <span>{{ formatBytes(batch.syncedSize) }} / {{ formatBytes(batch.totalSize) }}</span>
          <div class="sync-progress-cell"><div class="meter sync-meter"><span :style="{ width: batch.progress + '%' }"></span></div><strong>{{ batch.progress }}%</strong></div>
          <div v-if="batch.message" class="sync-message">{{ batch.message }}</div>
        </div>
        <div v-for="job in ufsSyncList" :key="job.path" class="sync-job">
          <div class="sync-target" :title="syncJobTitle(job)">
            <strong>{{ syncDisplayPath(job) }}</strong>
            <small>{{ syncJobSubtitle(job) }}</small>
          </div>
          <span :class="['sync-status', syncStatusClass(job)]">{{ syncState(job) }}</span>
          <span>{{ syncFiles(job) }}</span>
          <span>{{ syncBytes(job) }}</span>
          <div class="sync-progress-cell"><div class="meter sync-meter"><span :style="{ width: syncProgress(job) + '%' }"></span></div><strong>{{ syncProgress(job) }}%</strong></div>
          <div v-if="job.message" class="sync-message">{{ job.message }}</div>
        </div>
        </div>
      </div>
      <table class="admin-table file-browser-table">
        <colgroup><col class="name-col"><col class="type-col"><col class="cache-col"><col class="size-col"><col class="block-col"><col class="replica-col"><col class="modified-col"><col class="actions-col"></colgroup>
        <thead><tr><th>Name</th><th>Type</th><th>Cache</th><th>Size</th><th>Block Size</th><th>Replicas</th><th>Modified</th><th></th></tr></thead>
        <tbody><tr v-for="item in items" :key="item.path"><td class="file-name-cell" :title="item.path" @click="enterItem(item)"><div class="compact-name-cell"><strong>{{ fileName(item.path) }}</strong><button class="copy-path-button" title="Copy absolute path" @click.stop="copyPath(item.path)">{{ copiedPath === item.path ? 'Copied' : 'Copy' }}</button></div></td><td>{{ item.is_dir ? 'Directory' : 'File' }}</td><td><span :class="['cache-state-pill', cacheStateClass(item)]">{{ cacheState(item) }}</span></td><td>{{ item.is_dir ? '-' : formatBytes(item.len) }}</td><td>{{ item.block_size ? formatBytes(item.block_size) : '-' }}</td><td>{{ item.replicas || '-' }}</td><td>{{ formatTime(item.mtime) }}</td><td class="row-actions"><button class="link-button" @click="item.is_dir ? openDir(item.path) : openBlocks(item.path)">{{ item.is_dir ? 'Open' : 'Blocks' }}</button><button class="link-button" @click="downloadItem(item)">Download</button><button class="link-button" :disabled="!canFreeItem(item)" :title="freeItemTitle(item)" @click="freeItem(item)">Free</button><button class="link-button danger-link" :disabled="isMountRoot(item.path)" :title="deleteItemTitle(item)" @click="deleteItem(item)">Delete</button></td></tr></tbody>
      </table>
      <div v-if="items.length === 0 && !loading" class="empty-state">{{ cacheOnly ? 'No cached files in this path.' : 'Directory is empty.' }}</div>
    </section>
  </div>
</template>

<script>
import { createDirectory, deletePath, downloadFile, fetchBrowseData, fetchDirectoryCacheSummary, fetchMountsData, fetchUfsSyncJobs, fetchUfsSyncStatus, freePath, submitLoadJob, uploadFile } from '@/api/client'
import { formatBytes, formatTime } from '@/utils/format'
import eventBus from '@/utils/eventBus'

export default {
  name: 'FileSystemPage',
  data() { return { loading: false, browsePath: this.$route.query.path || '/', cacheOnly: false, items: [], copiedPath: '', uploadNotice: '', mounts: [], mountsLoaded: false, summaryLoadToken: 0, uploadBatches: {}, uploadBatchStorageRefreshTimer: null, ufsSyncJobs: {}, ufsSyncTimers: {}, ufsSyncStorageRefreshTimer: null, ufsSyncDiscoverTimer: null, ufsSyncDiscoverToken: 0, ufsSyncDismissedAt: {}, ufsSyncPanelExpanded: true } },
  computed: {
    uploadBatchList() { return Object.values(this.uploadBatches) },
    hasCompletedUploadBatches() {
      return Object.values(this.uploadBatches).some((batch) => batch && batch.done)
    },
    ufsSyncList() { return Object.values(this.ufsSyncJobs).filter((job) => !job.batch_id) },
    hasCompletedUfsSyncJobs() {
      return Object.values(this.ufsSyncJobs).some((job) => job && job.done)
    },
    ufsSyncJobCount() {
      return this.ufsSyncList.length + this.ufsSyncBatchList.length
    },
    ufsSyncActiveCount() {
      return Object.values(this.ufsSyncJobs).filter((job) => {
        if (!job) return false
        return !job.done && !this.isTerminalSyncState(this.syncState(job))
      }).length
    },
    ufsSyncSummaryText() {
      const total = this.ufsSyncJobCount
      const active = this.ufsSyncActiveCount
      if (!total) return ''
      if (active > 0) return `${active} active · ${total} total`
      return `${total} tracked`
    },
    ufsSyncBatchList() {
      const batches = {}
      for (const job of Object.values(this.ufsSyncJobs)) {
        if (!job.batch_id) continue
        const firstPath = job.path || job.source_path || ''
        const batch = batches[job.batch_id] || { id: job.batch_id, label: job.batch_label || this.syncDisplayPath(job), subtitle: firstPath, title: firstPath, totalFiles: 0, syncedFiles: 0, terminalFiles: 0, uploadFiles: 0, waitingFiles: 0, syncingFiles: 0, failedFiles: 0, syncedSize: 0, totalSize: 0, progressSize: 0, state: 'Queued', message: '' }
        const state = this.syncState(job)
        const progress = this.syncProgress(job)
        const totalSize = Number(job.total_size || 0)
        const totalFiles = Math.max(1, Number(job.total_files || 0))
        const completedFiles = state === 'Completed' ? totalFiles : Number(job.completed_files || 0)
        batch.totalFiles += totalFiles
        batch.syncedFiles += Math.max(0, Math.min(totalFiles, completedFiles))
        batch.totalSize += totalSize
        batch.progressSize += totalSize > 0 ? totalSize * progress / 100 : 0
        if (state === 'Completed') {
          batch.syncedSize += totalSize || Number(job.loaded_size || 0)
        }
        if (['Uploading to Curvine', 'Queued'].includes(state)) batch.uploadFiles += 1
        else if (['Waiting for UFS sync', 'Checking UFS sync', 'NotStarted'].includes(state)) batch.waitingFiles += 1
        else if (['Pending', 'Loading'].includes(state)) batch.syncingFiles += 1
        if (Boolean(job.done) || this.isTerminalSyncState(state)) batch.terminalFiles += 1
        if (['Failed', 'Canceled', 'Unknown', 'Timed out'].includes(state)) batch.failedFiles += 1
        if (!batch.message && job.message) batch.message = job.message
        batches[job.batch_id] = batch
      }
      return Object.values(batches).map((batch) => {
        const progress = batch.totalSize > 0
          ? Math.max(0, Math.min(100, Math.round(batch.progressSize / batch.totalSize * 100)))
          : (batch.syncedFiles === batch.totalFiles ? 100 : 0)
        if (batch.syncedFiles >= batch.totalFiles) return { ...batch, state: 'Completed', progress: 100 }
        if (batch.failedFiles > 0) return { ...batch, state: 'Needs attention', progress }
        if (batch.syncingFiles > 0) return { ...batch, state: 'Syncing to UFS', progress }
        if (batch.waitingFiles > 0) return { ...batch, state: 'Waiting for UFS sync', progress }
        if (batch.uploadFiles > 0) return { ...batch, state: 'Uploading to Curvine', progress }
        if (batch.terminalFiles >= batch.totalFiles) return { ...batch, state: 'Finished with skipped files', progress }
        return { ...batch, progress }
      })
    },
    canGoUp() { return !this.isRootPath },
    isRootPath() { return this.normalizePath(this.browsePath) === '/' },
    canFreeCurrentPath() { return !this.isRootPath && this.isMountedPath(this.browsePath) && this.items.some((item) => this.canFreeItem(item)) },
    currentFreeTitle() {
      if (this.isRootPath) return 'Root path cannot be freed'
      return this.canFreeCurrentPath ? 'Free cached data under this mounted path' : 'No UFS-backed cached data under this path can be freed'
    }
  },
  watch: {
    '$route.query.path'(path) { this.browsePath = path || '/'; this.restoreBrowseSnapshot(); this.fetchData() },
    ufsSyncActiveCount(newCount, oldCount) {
      if (newCount > oldCount && newCount > 0 && !this.ufsSyncPanelExpanded) {
        this.ufsSyncPanelExpanded = true
        this.saveUfsSyncPanelExpanded()
      }
    }
  },
  created() {
    this.restoreUploadBatches()
    this.loadUfsSyncDismissed()
    this.loadUfsSyncPanelExpanded()
    this.restoreUfsSyncJobs()
    this.restoreBrowseSnapshot()
    this.fetchData()
  },
  mounted() {
    eventBus.on('admin-refresh', this.fetchData)
    this.startUploadBatchStorageRefresh()
    this.startUfsSyncStorageRefresh()
    this.startUfsSyncDiscover()
  },
  beforeUnmount() {
    eventBus.off('admin-refresh', this.fetchData)
    Object.values(this.ufsSyncTimers).forEach((timer) => window.clearTimeout(timer))
    if (this.uploadBatchStorageRefreshTimer) window.clearTimeout(this.uploadBatchStorageRefreshTimer)
    if (this.ufsSyncStorageRefreshTimer) window.clearTimeout(this.ufsSyncStorageRefreshTimer)
    this.stopUfsSyncDiscover()
  },
  methods: {
    formatBytes, formatTime,
    async fetchData() {
      const token = this.summaryLoadToken + 1
      this.summaryLoadToken = token
      this.restoreBrowseSnapshot()
      this.loading = true
      try {
        await this.ensureMounts()
        const data = await fetchBrowseData({
          path: this.normalizePath(this.browsePath),
          cache_only: this.cacheOnly,
          include_dir_summary: this.cacheOnly
        })
        this.items = Array.isArray(data) ? data : (data.items || [])
        this.saveBrowseSnapshot()
        this.fetchVisibleUfsSyncJobs(token, { includeFinished: false })
        this.fetchVisiblePathUfsSyncStatuses(token)
        if (!this.cacheOnly) this.loadDirectoryCacheSummaries(token)
      } finally { this.loading = false }
    },
    browseSnapshotStorageKey() {
      return `curvine.web.fs.browseSnapshot.v1:${this.cacheOnly ? 'cache' : 'all'}:${this.normalizePath(this.browsePath)}`
    },
    saveBrowseSnapshot() {
      try {
        window.localStorage.setItem(this.browseSnapshotStorageKey(), JSON.stringify({
          items: this.items,
          saved_at: Date.now()
        }))
      } catch (_) {}
    },
    restoreBrowseSnapshot() {
      try {
        const raw = window.localStorage.getItem(this.browseSnapshotStorageKey())
        if (!raw) return
        const snapshot = JSON.parse(raw)
        if (!snapshot || !Array.isArray(snapshot.items)) return
        if (Date.now() - Number(snapshot.saved_at || 0) > 10 * 60 * 1000) return
        this.items = snapshot.items
      } catch (_) {}
    },
    async fetchVisibleUfsSyncJobs(token, options = {}) {
      const pathPrefix = options.pathPrefix || this.normalizePath(this.browsePath)
      const limit = options.limit || 100
      const includeFinished = options.includeFinished !== undefined ? options.includeFinished : true
      const includeTasks = options.includeTasks !== undefined ? options.includeTasks : false
      try {
        const data = await fetchUfsSyncJobs(pathPrefix, limit, includeFinished, { includeTasks })
        if (token !== this.summaryLoadToken && token !== this.ufsSyncDiscoverToken) return
        const jobs = Array.isArray(data) ? data : (data.items || [])
        for (const job of jobs) {
          if (!job || !job.path) continue
          this.mergeUfsSyncJobFromServer(this.normalizePath(job.path), job)
        }
      } catch (_) {}
    },
    async fetchVisiblePathUfsSyncStatuses(token) {
      const visibleFiles = this.items
        .filter((item) => item && !item.is_dir && this.isFsModePath(item.path))
        .slice(0, 100)
      for (const item of visibleFiles) {
        if (token !== this.summaryLoadToken && token !== this.ufsSyncDiscoverToken) return
        try {
          const job = await fetchUfsSyncStatus(item.path)
          if (token !== this.summaryLoadToken && token !== this.ufsSyncDiscoverToken) return
          if (!job || !job.path) continue
          const normalizedPath = this.normalizePath(job.path)
          const state = this.syncState(job)
          const hasExisting = Boolean(this.ufsSyncJobs[normalizedPath])
          if (!hasExisting && ['NotStarted', 'NotMounted', 'Unsupported'].includes(state)) continue
          this.mergeUfsSyncJobFromServer(normalizedPath, job)
        } catch (_) {}
      }
    },
    ufsSyncPanelStorageKey() {
      return 'curvine.web.fs.ufsSyncPanelExpanded.v1'
    },
    loadUfsSyncPanelExpanded() {
      try {
        const raw = window.localStorage.getItem(this.ufsSyncPanelStorageKey())
        this.ufsSyncPanelExpanded = raw === null ? true : raw === '1'
      } catch (_) {
        this.ufsSyncPanelExpanded = true
      }
    },
    saveUfsSyncPanelExpanded() {
      try {
        window.localStorage.setItem(this.ufsSyncPanelStorageKey(), this.ufsSyncPanelExpanded ? '1' : '0')
      } catch (_) {}
    },
    toggleUfsSyncPanel() {
      this.ufsSyncPanelExpanded = !this.ufsSyncPanelExpanded
      this.saveUfsSyncPanelExpanded()
    },
    ufsSyncDismissedStorageKey() {
      return 'curvine.web.fs.ufsSyncDismissed.v1'
    },
    loadUfsSyncDismissed() {
      try {
        const raw = window.localStorage.getItem(this.ufsSyncDismissedStorageKey())
        this.ufsSyncDismissedAt = raw ? (JSON.parse(raw) || {}) : {}
      } catch (_) {
        this.ufsSyncDismissedAt = {}
      }
    },
    saveUfsSyncDismissed() {
      try {
        window.localStorage.setItem(this.ufsSyncDismissedStorageKey(), JSON.stringify(this.ufsSyncDismissedAt))
      } catch (_) {}
    },
    isUfsSyncDismissed(path, job) {
      const normalizedPath = this.normalizePath(path)
      const dismissedAt = Number(this.ufsSyncDismissedAt[normalizedPath] || 0)
      if (!dismissedAt) return false
      if (!job) return true
      const state = job.state || job.status || ''
      const done = Boolean(job.done) || this.isTerminalSyncState(state)
      if (!done) return false
      const serverUpdated = Number(job.update_time_ms || job.updated_at || 0)
      return !serverUpdated || serverUpdated <= dismissedAt
    },
    markUfsSyncDismissed(path, job) {
      const normalizedPath = this.normalizePath(path)
      const now = Date.now()
      const stamp = Math.max(
        Number(this.ufsSyncDismissedAt[normalizedPath] || 0),
        Number(job && (job.update_time_ms || job.updated_at) || 0),
        now
      )
      this.ufsSyncDismissedAt = { ...this.ufsSyncDismissedAt, [normalizedPath]: stamp }
      this.saveUfsSyncDismissed()
    },
    mergeUfsSyncJobFromServer(path, job) {
      if (this.isUfsSyncDismissed(path, job)) return
      const existing = this.ufsSyncJobs[path] || {}
      const state = job.state || job.status || ''
      const done = Boolean(job.done) || this.isTerminalSyncState(state)
      const localState = this.syncState(existing)
      if (localState === 'Uploading to Curvine' && !done) {
        this.setUfsSyncJob(path, { ...existing, ...job, path, done: Boolean(existing.done) })
        return
      }
      const serverUpdated = Number(job.update_time_ms || job.updated_at || 0)
      const localUpdated = Number(existing.updated_at || 0)
      if (serverUpdated && localUpdated && serverUpdated < localUpdated && !done) return
      this.setUfsSyncJob(path, { ...job, path, done })
      if (!done && !this.ufsSyncTimers[path]) this.pollUfsSync(path)
    },
    startUfsSyncDiscover() {
      this.stopUfsSyncDiscover()
      const tick = async () => {
        const token = this.ufsSyncDiscoverToken
        await this.fetchVisibleUfsSyncJobs(token, { pathPrefix: '/', limit: 200, includeFinished: false })
        await this.fetchVisiblePathUfsSyncStatuses(token)
        if (token === this.ufsSyncDiscoverToken) {
          this.ufsSyncDiscoverTimer = window.setTimeout(tick, 4000)
        }
      }
      this.ufsSyncDiscoverTimer = window.setTimeout(tick, 500)
    },
    stopUfsSyncDiscover() {
      if (this.ufsSyncDiscoverTimer) {
        window.clearTimeout(this.ufsSyncDiscoverTimer)
        this.ufsSyncDiscoverTimer = null
      }
    },
    async loadDirectoryCacheSummaries(token) {
      const dirs = this.items.filter((item) => item && item.is_dir)
      for (const item of dirs) {
        if (token !== this.summaryLoadToken) return
        if (this.xAttrText(item, 'cache_state_summary')) continue
        try {
          const data = await fetchDirectoryCacheSummary(item.path)
          if (token !== this.summaryLoadToken) return
          this.setDirectoryCacheSummary(item.path, data && data.summary ? data.summary : 'Unknown')
        } catch (error) {
          if (token !== this.summaryLoadToken) return
          this.setDirectoryCacheSummary(item.path, `Unknown: ${error}`)
        }
      }
    },
    setDirectoryCacheSummary(path, summary) {
      const target = this.items.find((item) => item && item.path === path)
      if (!target) return
      target.x_attr = { ...(target.x_attr || {}), cache_state_summary: summary }
    },
    async mkdirDialog() {
      const path = window.prompt('Directory path', this.joinPath(this.browsePath, 'new-dir'))
      if (!path) return
      this.loading = true
      try {
        await createDirectory(path, true)
        window.alert(`Created directory: ${path}`)
        await this.fetchData()
      } catch (error) {
        window.alert(`Mkdir failed: ${error}`)
      } finally { this.loading = false }
    },
    async deleteItem(item) {
      if (this.isMountRoot(item.path)) {
        window.alert(this.deleteItemTitle(item))
        return
      }
      const recursive = Boolean(item.is_dir)
      const message = this.isMountedPath(item.path)
        ? `${recursive ? 'Delete directory' : 'Delete file'} ${item.path}? This also deletes the backing UFS data.`
        : `${recursive ? 'Delete directory' : 'Delete file'} ${item.path}?`
      if (!window.confirm(message)) return
      this.loading = true
      try {
        await deletePath(item.path, recursive)
        window.alert(`Deleted: ${item.path}`)
        await this.fetchData()
      } catch (error) {
        window.alert(`Delete failed: ${error}`)
      } finally { this.loading = false }
    },
    uploadFileDialog() {
      this.$refs.fileInput.value = ''
      this.$refs.fileInput.click()
    },
    uploadFolderDialog() {
      this.$refs.folderInput.value = ''
      this.$refs.folderInput.click()
    },
    async onFilePicked(event) {
      const file = event.target.files && event.target.files[0]
      if (!file) return
      const remotePath = this.joinPath(this.browsePath, file.name)
      this.uploadNotice = ''
      this.setUfsSyncJob(remotePath, {
        path: remotePath,
        state: 'Uploading to Curvine',
        status: 'Uploading to Curvine',
        progress: 0,
        total_files: 1,
        completed_files: 0,
        loaded_size: 0,
        total_size: Number(file.size || 0),
        done: false,
        attempts: 0
      })
      try {
        const result = await uploadFile(remotePath, file, true, (progressEvent) => {
          const total = Number(progressEvent.total || file.size || 0)
          const loaded = Number(progressEvent.loaded || 0)
          this.setUfsSyncJob(remotePath, {
            state: 'Uploading to Curvine',
            status: 'Uploading to Curvine',
            loaded_size: loaded,
            total_size: total,
            total_files: 1,
            completed_files: 0,
            done: false
          })
        })
        if (result && result.ufs_sync) {
          this.setUfsSyncJob(remotePath, {
            ...(result.ufs_sync || {}),
            state: (result.ufs_sync && (result.ufs_sync.state || result.ufs_sync.status)) || 'Waiting for UFS sync',
            status: (result.ufs_sync && (result.ufs_sync.status || result.ufs_sync.state)) || 'Waiting for UFS sync',
            loaded_size: Number(file.size || 0),
            total_size: Number(file.size || 0),
            total_files: 1,
            completed_files: 0,
            done: false
          })
          this.trackUfsSync(remotePath)
        } else {
          this.removeUfsSyncJob(remotePath)
        }
        this.uploadNotice = `Uploaded: ${remotePath}`
        await this.fetchData()
      } catch (error) {
        window.alert(`Upload failed: ${error}`)
        this.setUfsSyncJob(remotePath, { state: 'Upload failed', status: 'Upload failed', message: String(error), done: true })
      }
    },
    async onFolderPicked(event) {
      const files = Array.from((event.target && event.target.files) || [])
      if (files.length === 0) return
      const folderName = files[0].webkitRelativePath ? files[0].webkitRelativePath.split('/')[0] : 'Folder upload'
      await this.uploadFolderBatch(files.map((file) => ({ file, relativePath: file.webkitRelativePath || file.name })), folderName)
    },
    async uploadFolderBatch(fileItems, folderName) {
      const batchId = `folder-${Date.now()}`
      const totalFiles = fileItems.length
      const totalSize = fileItems.reduce((sum, item) => sum + Number(item.file && item.file.size ? item.file.size : 0), 0)
      this.uploadNotice = `Uploading ${totalFiles} file(s) to Curvine...`
      this.setUploadBatch(batchId, {
        label: folderName || 'Folder upload',
        state: 'Uploading to Curvine',
        totalFiles,
        uploadedFiles: 0,
        uploadedSize: 0,
        totalSize,
        progress: 0,
        done: false,
        message: ''
      })

      let uploadedFiles = 0
      let uploadedSize = 0
      let lastUiUpdateMs = 0
      try {
        for (const { file, relativePath } of fileItems) {
          const remotePath = this.normalizePath(this.joinPath(this.browsePath, relativePath))
          await uploadFile(remotePath, file, true, null, 300000, false)
          uploadedFiles += 1
          uploadedSize += Number(file.size || 0)
          const now = Date.now()
          if (now - lastUiUpdateMs >= 500 || uploadedFiles === totalFiles) {
            lastUiUpdateMs = now
            this.setUploadBatch(batchId, {
              state: uploadedFiles === totalFiles ? 'Uploaded to Curvine' : 'Uploading to Curvine',
              uploadedFiles,
              uploadedSize,
              progress: totalFiles > 0 ? Math.max(0, Math.min(100, Math.round(uploadedFiles / totalFiles * 100))) : 100,
              done: uploadedFiles === totalFiles
            })
            await this.nextUiFrame()
          }
        }

        this.uploadNotice = `Uploaded ${uploadedFiles} file(s) from folder.`
        await this.submitFolderUfsSyncIfNeeded(batchId, folderName || 'Folder upload')
        await this.fetchData()
      } catch (error) {
        this.setUploadBatch(batchId, {
          state: 'Upload failed',
          uploadedFiles,
          uploadedSize,
          progress: totalFiles > 0 ? Math.max(0, Math.min(100, Math.round(uploadedFiles / totalFiles * 100))) : 0,
          done: true,
          message: String(error)
        })
        window.alert(`Folder upload failed: ${error}`)
      }
    },
    async submitFolderUfsSyncIfNeeded(batchId, folderName) {
      const folderPath = this.normalizePath(this.joinPath(this.browsePath, folderName))
      await this.ensureMounts()
      if (!this.isFsModePath(folderPath)) return

      try {
        const result = await submitLoadJob({ path: folderPath, recursive: true, overwrite: true })
        const syncJob = (result && result.ufs_sync) || result || {}
        const state = syncJob.state || syncJob.status || 'Pending'
        this.setUfsSyncJob(folderPath, {
          ...syncJob,
          path: folderPath,
          batch_id: batchId,
          batch_label: folderName,
          state,
          status: syncJob.status || state,
          progress: Number(syncJob.progress || 0),
          total_files: Number(syncJob.total_files || 0),
          completed_files: Number(syncJob.completed_files || 0),
          loaded_size: Number(syncJob.loaded_size || 0),
          total_size: Number(syncJob.total_size || 0),
          done: this.isTerminalSyncState(state),
          attempts: 0
        })
        if (!this.isTerminalSyncState(state)) this.trackUfsSync(folderPath, batchId, folderName)
      } catch (error) {
        this.setUfsSyncJob(folderPath, {
          path: folderPath,
          batch_id: batchId,
          batch_label: folderName,
          state: 'Failed',
          status: 'Failed',
          message: String(error),
          done: true
        })
      }
    },

    async downloadItem(item) {
      this.loading = true
      try {
        const response = await downloadFile(item.path)
        const blob = response.data
        const url = window.URL.createObjectURL(blob)
        const link = document.createElement('a')
        link.href = url
        link.download = item.is_dir ? `${this.fileName(item.path)}.tar` : this.fileName(item.path)
        document.body.appendChild(link)
        link.click()
        document.body.removeChild(link)
        window.URL.revokeObjectURL(url)
      } catch (error) {
        window.alert(`Download failed: ${error}`)
      } finally { this.loading = false }
    },
    async freeItem(item) {
      if (!this.canFreeItem(item)) {
        window.alert(this.freeItemTitle(item))
        return
      }
      const recursive = Boolean(item.is_dir)
      const message = recursive ? `Free cached data under ${item.path} recursively?` : `Free cached data for ${item.path}?`
      if (!window.confirm(message)) return
      await this.freePath(item.path, recursive)
    },
    async freeCurrentPath() {
      const path = this.normalizePath(this.browsePath)
      if (!this.canFreeCurrentPath) {
        window.alert(this.currentFreeTitle)
        return
      }
      if (!window.confirm(`Free cached data under ${path} recursively?`)) return
      await this.freePath(path, true)
    },
    async freePath(path, recursive) {
      this.loading = true
      try {
        const data = await freePath(path, recursive)
        const bytes = data.bytes || 0
        const inodes = data.inodes || 0
        if (bytes === 0 && inodes === 0) {
          window.alert('No cached data to free. The selected path is already UFS-only or has no cached descendants.')
        } else {
          window.alert(`Freed ${this.formatBytes(bytes)} from ${inodes} inode(s)`)
        }
        await this.fetchData()
      } catch (error) {
        window.alert(`Free failed: ${error}`)
      } finally { this.loading = false }
    },
    setUploadBatch(batchId, patch) {
      const previous = this.uploadBatches[batchId] || {}
      this.uploadBatches = { ...this.uploadBatches, [batchId]: { ...previous, ...patch, id: batchId, updated_at: Date.now() } }
      this.saveUploadBatches()
    },
    clearCompletedUploadBatches() {
      const next = {}
      for (const [id, batch] of Object.entries(this.uploadBatches)) {
        if (!batch || batch.done) continue
        next[id] = batch
      }
      this.uploadBatches = next
      this.writeUploadBatches(next)
    },
    clearUploadBatches() {
      this.uploadBatches = {}
      this.writeUploadBatches({})
    },
    uploadBatchStaleMs() {
      return 10 * 60 * 1000
    },
    normalizeUploadBatch(batch, now = Date.now()) {
      if (!batch) return batch
      const updatedAt = Number(batch.updated_at || 0)
      if (batch.done || updatedAt <= 0 || now - updatedAt <= this.uploadBatchStaleMs()) return batch
      return {
        ...batch,
        state: 'Interrupted',
        done: true,
        message: 'Upload progress stopped updating. The browser upload was interrupted or the page was refreshed.'
      }
    },
    uploadBatchStorageKey() {
      return 'curvine.web.fs.uploadBatches.v1'
    },
    writeUploadBatches(batches) {
      try {
        window.localStorage.setItem(this.uploadBatchStorageKey(), JSON.stringify(batches))
      } catch (_) {}
    },
    readStoredUploadBatches() {
      try {
        const raw = window.localStorage.getItem(this.uploadBatchStorageKey())
        return raw ? (JSON.parse(raw) || {}) : {}
      } catch (_) {
        return {}
      }
    },
    saveUploadBatches() {
      try {
        const now = Date.now()
        const keepMs = 24 * 60 * 60 * 1000
        const stored = this.readStoredUploadBatches()
        const batches = { ...stored }
        for (const [id, batch] of Object.entries(this.uploadBatches)) {
          const normalized = this.normalizeUploadBatch(batch, now)
          const currentUpdatedAt = Number(normalized.updated_at || 0)
          const storedUpdatedAt = Number(batches[id]?.updated_at || 0)
          if (now - Number(normalized.updated_at || now) <= keepMs && currentUpdatedAt >= storedUpdatedAt) batches[id] = normalized
        }
        for (const [id, batch] of Object.entries(batches)) {
          const normalized = this.normalizeUploadBatch(batch, now)
          if (!normalized || now - Number(normalized.updated_at || now) > keepMs) delete batches[id]
          else batches[id] = normalized
        }
        this.writeUploadBatches(batches)
      } catch (_) {}
    },
    restoreUploadBatches() {
      try {
        const now = Date.now()
        const keepMs = 24 * 60 * 60 * 1000
        const restored = this.readStoredUploadBatches()
        const batches = {}
        for (const [id, batch] of Object.entries(restored)) {
          if (!batch || now - Number(batch.updated_at || now) > keepMs) continue
          batches[id] = this.normalizeUploadBatch(batch, now)
        }
        this.uploadBatches = batches
        this.writeUploadBatches(batches)
      } catch (_) {
        this.uploadBatches = {}
      }
    },
    startUploadBatchStorageRefresh() {
      if (this.uploadBatchStorageRefreshTimer) window.clearTimeout(this.uploadBatchStorageRefreshTimer)
      const refresh = () => {
        this.refreshUploadBatchesFromStorage()
        this.uploadBatchStorageRefreshTimer = window.setTimeout(refresh, 1000)
      }
      this.uploadBatchStorageRefreshTimer = window.setTimeout(refresh, 1000)
    },
    refreshUploadBatchesFromStorage() {
      const stored = this.readStoredUploadBatches()
      const next = { ...this.uploadBatches }
      let changed = false
      const now = Date.now()
      for (const [id, batch] of Object.entries(stored)) {
        if (!batch) continue
        const normalized = this.normalizeUploadBatch(batch, now)
        const currentUpdatedAt = Number(next[id]?.updated_at || 0)
        const storedUpdatedAt = Number(normalized.updated_at || 0)
        if (!next[id] || storedUpdatedAt > currentUpdatedAt || next[id].state !== normalized.state || next[id].done !== normalized.done) {
          next[id] = normalized
          changed = true
        }
      }
      if (!changed) return
      this.uploadBatches = next
      this.writeUploadBatches(next)
    },
    nextUiFrame() {
      return new Promise((resolve) => window.requestAnimationFrame(() => resolve()))
    },
    trackUfsSync(path, batchId = '', batchLabel = '') {
      const normalizedPath = this.normalizePath(path)
      const previous = this.ufsSyncJobs[normalizedPath] || {}
      this.setUfsSyncJob(normalizedPath, {
        path: normalizedPath,
        batch_id: batchId,
        batch_label: batchLabel,
        state: 'Checking UFS sync',
        status: 'Checking UFS sync',
        progress: Number(previous.progress || 0),
        total_files: Number(previous.total_files || 0),
        completed_files: Number(previous.completed_files || 0),
        loaded_size: Number(previous.loaded_size || 0),
        total_size: Number(previous.total_size || 0),
        done: false,
        attempts: 0
      })
      this.pollUfsSync(normalizedPath)
    },
    setUfsSyncJob(path, job) {
      const normalizedPath = this.normalizePath(path)
      const nextJob = { ...(this.ufsSyncJobs[normalizedPath] || {}), ...job, path: normalizedPath, updated_at: Date.now() }
      if (this.isUfsSyncDismissed(normalizedPath, nextJob)) return
      this.ufsSyncJobs = { ...this.ufsSyncJobs, [normalizedPath]: nextJob }
      this.saveUfsSyncJobs()
    },
    ufsSyncStorageKey() {
      return 'curvine.web.fs.ufsSyncJobs.v3'
    },
    saveUfsSyncJobs() {
      try {
        const now = Date.now()
        const keepMs = 30 * 60 * 1000
        const stored = this.readStoredUfsSyncJobs()
        const jobs = { ...stored }
        for (const [path, job] of Object.entries(this.ufsSyncJobs)) {
          const state = this.syncState(job)
          if (Boolean(job.done) || this.isTerminalSyncState(state)) {
            delete jobs[path]
            continue
          }
          const currentUpdatedAt = Number(job.updated_at || 0)
          const storedUpdatedAt = Number(jobs[path]?.updated_at || 0)
          if (now - Number(job.updated_at || now) <= keepMs && currentUpdatedAt >= storedUpdatedAt) jobs[path] = job
        }
        for (const [path, job] of Object.entries(jobs)) {
          const state = this.syncState(job)
          if (!job || Boolean(job.done) || this.isTerminalSyncState(state) || now - Number(job.updated_at || now) > keepMs || this.isUfsSyncDismissed(path, job)) delete jobs[path]
        }
        window.localStorage.setItem(this.ufsSyncStorageKey(), JSON.stringify(jobs))
      } catch (_) {}
    },
    restoreUfsSyncJobs() {
      try {
        const raw = window.localStorage.getItem(this.ufsSyncStorageKey())
        if (!raw) return
        const now = Date.now()
        const keepMs = 24 * 60 * 60 * 1000
        const restored = JSON.parse(raw) || {}
        const jobs = {}
        for (const [path, job] of Object.entries(restored)) {
          if (!job || now - Number(job.updated_at || now) > keepMs) continue
          if (this.isUfsSyncDismissed(path, job)) continue
          const state = this.syncState(job)
          if (Boolean(job.done) || this.isTerminalSyncState(state)) continue
          jobs[path] = job
        }
        this.ufsSyncJobs = jobs
        this.saveUfsSyncJobs()
        for (const [path, job] of Object.entries(jobs)) {
          const state = this.syncState(job)
          if (!job.done && state !== 'Queued') this.pollUfsSync(path)
        }
      } catch (_) {
        this.ufsSyncJobs = {}
      }
    },
    readStoredUfsSyncJobs() {
      try {
        const raw = window.localStorage.getItem(this.ufsSyncStorageKey())
        return raw ? (JSON.parse(raw) || {}) : {}
      } catch (_) {
        return {}
      }
    },
    startUfsSyncStorageRefresh() {
      if (this.ufsSyncStorageRefreshTimer) window.clearTimeout(this.ufsSyncStorageRefreshTimer)
      const refresh = () => {
        this.refreshUfsSyncJobsFromStorage()
        this.ufsSyncStorageRefreshTimer = window.setTimeout(refresh, 1000)
      }
      this.ufsSyncStorageRefreshTimer = window.setTimeout(refresh, 1000)
    },
    refreshUfsSyncJobsFromStorage() {
      const stored = this.readStoredUfsSyncJobs()
      const next = { ...this.ufsSyncJobs }
      let changed = false
      for (const [path, job] of Object.entries(stored)) {
        if (!job) continue
        if (this.isUfsSyncDismissed(path, job)) continue
        const state = this.syncState(job)
        if (Boolean(job.done) || this.isTerminalSyncState(state)) continue
        const currentUpdatedAt = Number(next[path]?.updated_at || 0)
        const storedUpdatedAt = Number(job.updated_at || 0)
        if (!next[path] || storedUpdatedAt > currentUpdatedAt) {
          next[path] = job
          changed = true
        }
      }
      if (!changed) return
      this.ufsSyncJobs = next
      for (const [path, job] of Object.entries(next)) {
        const state = this.syncState(job)
        if (!job.done && !this.ufsSyncTimers[path] && state !== 'Queued') {
          this.pollUfsSync(path)
        }
      }
    },
    async pollUfsSync(path) {
      if (this.ufsSyncTimers[path]) {
        window.clearTimeout(this.ufsSyncTimers[path])
        delete this.ufsSyncTimers[path]
      }
      const previous = this.ufsSyncJobs[path] || { attempts: 0 }
      try {
        const data = await fetchUfsSyncStatus(path)
        const attempts = (previous.attempts || 0) + 1
        const done = Boolean(data && data.done) || this.isTerminalSyncState(data && (data.state || data.status))
        this.setUfsSyncJob(path, { ...(data || {}), attempts, done })
        if (!done && attempts < 240) {
          this.ufsSyncTimers[path] = window.setTimeout(() => this.pollUfsSync(path), 1500)
        } else if (!done) {
          this.setUfsSyncJob(path, { state: 'Timed out', status: 'Timed out', message: 'UFS sync status did not finish after 6 minutes. Refresh status or check the load job.', done: true })
        }
      } catch (error) {
        const message = String(error)
        if (message.includes('Request URI Not Found') || message.includes('Expected JSON API response')) {
          this.setUfsSyncJob(path, { state: 'Unsupported', status: 'Unsupported', message: 'The running web API does not expose /v1/fs/ufs-sync. Rebuild or restart the web service.', done: true })
          return
        }
        this.setUfsSyncJob(path, { state: 'Unknown', status: 'Unknown', message, done: true })
      }
    },
    syncState(job) { return job.state || job.status || 'Unknown' },
    syncDisplayPath(job) {
      const path = this.normalizePath(job.path || job.source_path || '')
      const current = this.normalizePath(this.browsePath)
      if (!path) return '-'
      if (path === current) return path
      if (current !== '/' && path.startsWith(`${current}/`)) return path.slice(current.length + 1)
      return path
    },
    syncJobTitle(job) {
      const source = job.source_path || job.path || ''
      const target = job.target_path || job.ufs_path || ''
      const id = job.job_id || job.id || ''
      return [source && `CV: ${source}`, target && `UFS: ${target}`, id && `Job: ${id}`].filter(Boolean).join('\n')
    },
    syncJobSubtitle(job) {
      const target = job.target_path || job.ufs_path || ''
      const id = this.shortJobId(job.job_id || job.id)
      if (target && id) return `${target} · ${id}`
      if (target) return target
      if (id) return id
      return job.mount_path ? `mount ${job.mount_path}` : (job.source_path || job.path || '')
    },
    shortJobId(id) {
      if (!id) return ''
      const value = String(id)
      return value.length > 14 ? `${value.slice(0, 10)}...${value.slice(-4)}` : value
    },
    isTerminalSyncState(state) {
      return ['Completed', 'Failed', 'Canceled', 'Unsupported', 'NotMounted', 'Unknown', 'Timed out', 'Upload failed'].includes(String(state || ''))
    },
    syncProgress(job) {
      const state = this.syncState(job)
      if (state === 'Completed') return 100
      if (['Queued', 'NotStarted', 'Waiting for UFS sync', 'Checking UFS sync'].includes(state)) return 0
      const value = Number(job.progress || 0)
      if (value > 0 || ['Pending', 'Loading'].includes(state)) return Math.max(0, Math.min(100, Math.round(value)))
      const total = Number(job.total_size || 0)
      if (total <= 0) return 0
      return Math.max(0, Math.min(100, Math.round(Number(job.loaded_size || 0) / total * 100)))
    },
    syncBytes(job) {
      const state = this.syncState(job)
      const total = Number(job.total_size || 0)
      const loaded = state === 'Completed' && total > 0 ? total : Number(job.loaded_size || 0)
      return `${this.formatBytes(loaded)} / ${this.formatBytes(total)}`
    },
    syncFiles(job) {
      const total = Number(job.total_files || 0)
      const state = this.syncState(job)
      if (!total) {
        if (state === 'NotStarted') return 'waiting for job'
        return 'files pending'
      }
      return `${Number(job.completed_files || 0)} / ${total} files`
    },
    syncStatusClass(job) {
      const state = typeof job === 'string' ? job : this.syncState(job)
      if (['Completed'].includes(state)) return 'completed'
      if (['Failed', 'Canceled', 'Unknown', 'Timed out', 'Needs attention', 'Upload failed', 'Interrupted'].includes(state)) return 'failed'
      if (['Unsupported', 'NotMounted', 'Finished with skipped files'].includes(state)) return 'warning'
      if (['Uploading to Curvine', 'Pending', 'Loading', 'Syncing to UFS'].includes(state)) return 'running'
      return 'waiting'
    },
    clearCompletedUfsSyncJobs() {
      const next = {}
      for (const [path, job] of Object.entries(this.ufsSyncJobs)) {
        if (!job) continue
        const state = this.syncState(job)
        const terminal = Boolean(job.done) || this.isTerminalSyncState(state)
        if (terminal) {
          this.markUfsSyncDismissed(path, job)
          if (this.ufsSyncTimers[path]) {
            window.clearTimeout(this.ufsSyncTimers[path])
            delete this.ufsSyncTimers[path]
          }
          continue
        }
        next[path] = job
      }
      this.ufsSyncJobs = next
      try {
        window.localStorage.setItem(this.ufsSyncStorageKey(), JSON.stringify(next))
      } catch (_) {}
    },
    removeUfsSyncJob(path) {
      const normalizedPath = this.normalizePath(path)
      const next = { ...this.ufsSyncJobs }
      delete next[normalizedPath]
      this.ufsSyncJobs = next
      try {
        const stored = this.readStoredUfsSyncJobs()
        delete stored[normalizedPath]
        window.localStorage.setItem(this.ufsSyncStorageKey(), JSON.stringify(stored))
      } catch (_) {}
    },
    async copyPath(path) {
      try {
        if (navigator.clipboard && window.isSecureContext) {
          await navigator.clipboard.writeText(path)
        } else {
          const input = document.createElement('textarea')
          input.value = path
          input.setAttribute('readonly', '')
          input.style.position = 'fixed'
          input.style.opacity = '0'
          document.body.appendChild(input)
          input.select()
          document.execCommand('copy')
          document.body.removeChild(input)
        }
        this.copiedPath = path
        window.setTimeout(() => { if (this.copiedPath === path) this.copiedPath = '' }, 1600)
      } catch (error) {
        window.alert(`Copy failed: ${error}`)
      }
    },
    fileName(path) { const parts = String(path || '').split('/').filter(Boolean); return parts.pop() || '/' },
    joinPath(parent, name) { return `${String(parent || '/').replace(/\/$/, '')}/${name}`.replace(/^\/\//, '/') },
    async ensureMounts() {
      if (this.mountsLoaded) return
      try {
        const data = await fetchMountsData()
        this.mounts = Array.isArray(data) ? data : (data.items || [])
      } catch (_) {
        this.mounts = []
      } finally {
        this.mountsLoaded = true
      }
    },
    isPathInside(path, base) {
      const normalizedPath = this.normalizePath(path)
      const normalizedBase = this.normalizePath(base)
      return normalizedPath === normalizedBase || normalizedPath.startsWith(`${normalizedBase}/`)
    },
    writeTypeLabel(value) {
      if (value === 1 || value === 'FsMode') return 'fs_mode'
      if (value === 0 || value === 'CacheMode') return 'cache_mode'
      return String(value || 'fs_mode').toLowerCase()
    },
    isMountedPath(path) {
      return this.mounts.some((mount) => mount && mount.cv_path && this.isPathInside(path, mount.cv_path))
    },
    isFsModePath(path) {
      return this.mounts.some((mount) => mount && mount.cv_path && this.isPathInside(path, mount.cv_path) && this.writeTypeLabel(mount.write_type) === 'fs_mode')
    },
    isMountRoot(path) {
      const normalizedPath = this.normalizePath(path)
      return this.mounts.some((mount) => mount && mount.cv_path && normalizedPath === this.normalizePath(mount.cv_path))
    },
    deleteItemTitle(item) {
      if (item && this.isMountRoot(item.path)) return 'Use Mounts page to delete mount'
      if (item && this.isMountedPath(item.path)) return 'Delete from Curvine and backing UFS'
      return item && item.is_dir ? 'Delete directory recursively' : 'Delete file'
    },
    canFreeItem(item) {
      if (!item || !this.isMountedPath(item.path)) return false
      if (item.is_dir) return ['Cached', 'Mixed'].includes(this.cacheState(item))
      const policy = item.storage_policy || {}
      return policy.state === 'Both' && policy.ttl_action !== 'None'
    },
    freeItemTitle(item) {
      if (!item || !this.isMountedPath(item.path)) return 'Only paths under a UFS mount can be freed'
      if (item.is_dir) {
        const summary = this.cacheState(item)
        if (summary === 'Loading') return 'Cache state is still loading'
        if (summary === 'UFS only') return 'Already UFS only; no cached descendants remain'
        if (summary === 'CV only') return 'CV-only directory; not linked to UFS cached data'
        return 'Free cached data under this mounted directory'
      }
      const policy = item.storage_policy || {}
      if (policy.state === 'Ufs') return 'Already UFS only; no cached data remains'
      if (policy.state === 'Cv') return 'CV-only file; not linked to UFS cached data'
      if (policy.ttl_action === 'None') return 'TTL action is None, so this file cannot be freed'
      return 'Free cached data for this file'
    },
    cacheState(item) {
      if (item && item.is_dir) {
        return this.xAttrText(item, 'cache_state_summary') || 'Loading'
      }
      const state = item && item.storage_policy && item.storage_policy.state
      if (state === 'Both') return 'Cached'
      if (state === 'Ufs') return 'UFS only'
      if (state === 'Cv') return 'CV only'
      return 'Unknown'
    },
    cacheStateClass(item) {
      return String(this.cacheState(item)).toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')
    },
    xAttrText(item, key) {
      const value = item && item.x_attr && item.x_attr[key]
      if (!value) return ''
      if (typeof value === 'string') return value
      if (Array.isArray(value)) return String.fromCharCode(...value)
      return ''
    },
    normalizePath(path) {
      const value = String(path || '/').trim()
      if (!value || value === '/') return '/'
      return `/${value.replace(/^\/+|\/+$/g, '')}`.replace(/\/+/g, '/')
    },
    parentPath(path) {
      const parts = this.normalizePath(path).split('/').filter(Boolean)
      parts.pop()
      return parts.length ? `/${parts.join('/')}` : '/'
    },
    navigateTo(path) {
      const normalizedPath = this.normalizePath(path)
      this.browsePath = normalizedPath
      if (this.$route.path === '/browse' && (this.$route.query.path || '/') === normalizedPath) {
        this.fetchData()
        return
      }
      this.$router.push({ path: '/browse', query: { path: normalizedPath } })
    },
    navigate() { this.navigateTo(this.browsePath) },
    setRoot() { this.navigateTo('/') },
    goUp() { if (this.canGoUp) this.navigateTo(this.parentPath(this.browsePath)) },
    enterItem(item) { item.is_dir ? this.openDir(item.path) : this.openBlocks(item.path) },
    openDir(path) { this.navigateTo(path) },
    openBlocks(path) { this.$router.push({ path: '/blocks', query: { path } }) },
    goBlocks() { this.$router.push({ path: '/blocks', query: { path: this.browsePath } }) }
  }
}
</script>

<style scoped>
.cache-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: #44546a;
  font-size: 13px;
  white-space: nowrap;
}

.hidden-file-input {
  display: none;
}

 .cache-mode-note {
  color: #5b677a;
  font-size: 12px;
  font-weight: 700;
  white-space: nowrap;
}

.upload-notice {
  margin: 10px 0 0;
  color: #2f5d1f;
  font-size: 12px;
  font-weight: 700;
}

.sync-panel {
  display: grid;
  gap: 8px;
  margin: 10px 0 12px;
  padding: 10px 12px;
  border: 1px solid #dbe8d2;
  background: #f7fbf4;
  border-radius: 8px;
}

.sync-panel.ufs-sync-panel {
  display: flex;
  flex-direction: column;
  gap: 0;
}

.sync-panel.ufs-sync-panel.is-collapsed {
  margin-bottom: 8px;
}

.sync-panel-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  color: #24351f;
  font-size: 13px;
  font-weight: 800;
}

.sync-panel-header {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  padding-bottom: 2px;
}

.sync-panel.ufs-sync-panel:not(.is-collapsed) .sync-panel-header {
  margin-bottom: 8px;
  padding-bottom: 8px;
  border-bottom: 1px solid #e3eedc;
}

.sync-collapse-btn {
  flex-shrink: 0;
  width: 32px;
  height: 32px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  margin: 0;
  border: 1px solid #b8d4ae;
  border-radius: 8px;
  background: #fff;
  color: #2f5d1f;
  cursor: pointer;
  outline: none;
  box-shadow: 0 1px 2px rgba(36, 53, 31, 0.08);
  transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
  -webkit-tap-highlight-color: transparent;
}

.sync-collapse-btn:hover {
  background: #eef8e8;
  border-color: #7fb070;
}

.sync-collapse-btn:active {
  background: #e3f2dc;
  box-shadow: inset 0 1px 2px rgba(36, 53, 31, 0.08);
}

.sync-collapse-btn:focus {
  outline: none;
}

.sync-collapse-btn:focus-visible {
  outline: 2px solid #6aae5c;
  outline-offset: 2px;
}

.sync-collapse-icon {
  width: 14px;
  height: 14px;
  display: block;
  transition: transform 0.2s ease;
  transform: rotate(-90deg);
}

.sync-collapse-icon.expanded {
  transform: rotate(0deg);
}

.sync-panel-heading-main {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 8px 12px;
  cursor: pointer;
  border-radius: 6px;
  padding: 6px 8px;
  margin: -6px -8px;
  color: #24351f;
  outline: none;
  -webkit-tap-highlight-color: transparent;
}

.sync-panel-heading-main:hover {
  background: rgba(255, 255, 255, 0.72);
}

.sync-panel-heading-main:focus {
  outline: none;
}

.sync-panel-heading-main:focus-visible {
  box-shadow: 0 0 0 2px rgba(106, 174, 92, 0.45);
}

.sync-panel-toggle-label {
  font-size: 13px;
  font-weight: 800;
  line-height: 1.3;
}

.sync-panel-summary {
  color: #5b677a;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.3;
}

.sync-panel-body {
  display: grid;
  gap: 8px;
  max-height: min(280px, 40vh);
  overflow-y: auto;
}

.sync-title-actions {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.sync-job {
  display: grid;
  grid-template-columns: minmax(120px, 1fr) 118px 126px 150px minmax(120px, .65fr);
  gap: 12px;
  align-items: center;
  color: #44546a;
  font-size: 12px;
}

.sync-target {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sync-target > strong,
.sync-target > small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sync-target > strong {
  color: #24351f;
}

.sync-target > small {
  color: #6b7687;
  font-size: 11px;
  font-weight: 500;
}

.batch-sync {
  color: #24351f;
  font-weight: 700;
}

.sync-status {
  justify-self: start;
  border-radius: 999px;
  padding: 3px 8px;
  background: #eef2f7;
  color: #44546a;
  font-weight: 700;
  white-space: nowrap;
}

.sync-status.completed {
  background: #e8f7ee;
  color: #16794a;
}

.sync-status.running {
  background: #e8f1ff;
  color: #2356a3;
}

.sync-status.waiting {
  background: #fff4df;
  color: #97610a;
}

.sync-status.warning {
  background: #f2e8ff;
  color: #6d3db5;
}

.sync-status.failed {
  background: #f8e8e8;
  color: #a13d3d;
}

.sync-progress-cell {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.sync-progress-cell > strong {
  flex: 0 0 38px;
  text-align: right;
  color: #24351f;
}

.sync-meter {
  flex: 1 1 auto;
  min-width: 72px;
  margin: 0;
}

.sync-message {
  grid-column: 1 / -1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #7a4f09;
  font-weight: 500;
}

@media (max-width: 1100px) {
  .sync-job {
    grid-template-columns: minmax(0, 1fr) 118px;
  }

  .sync-progress-cell,
  .sync-message {
    grid-column: 1 / -1;
  }
}

.cache-col {
  width: 86px;
}

.cache-state-pill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 64px;
  padding: 3px 8px;
  border-radius: 999px;
  background: #eef2f7;
  color: #44546a;
  font-size: 12px;
  font-weight: 700;
}

.cache-state-pill.cached {
  background: #e8f7ee;
  color: #16794a;
}

.cache-state-pill.ufs-only {
  background: #fff4df;
  color: #97610a;
}

.cache-state-pill.cv-only {
  background: #eef2f7;
  color: #44546a;
}

.cache-state-pill.mixed {
  background: #f2e8ff;
  color: #6d3db5;
}

.cache-state-pill.unknown {
  background: #f8e8e8;
  color: #a13d3d;
}

.cache-state-pill.loading {
  background: #eef2f7;
  color: #5b677a;
}
</style>

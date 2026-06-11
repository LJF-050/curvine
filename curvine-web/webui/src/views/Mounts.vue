<template>
  <div class="admin-page" v-loading="loading">
    <section class="metric-grid">
      <MetricCard label="Mount Points" :value="mounts.length" meta="configured UFS mappings" delta="current" tone="good" />
      <MetricCard label="FS Mode" :value="fsModeCount" meta="metadata sync mounts" delta="resync" tone="neutral" />
      <MetricCard label="Cache Mode" :value="cacheModeCount" meta="UFS passthrough mounts" delta="cache" tone="neutral" />
      <MetricCard label="Providers" :value="providerCount" meta="detected UFS types" delta="auto" tone="neutral" />
    </section>

    <section class="content-grid mounts-grid">
      <div class="admin-panel table-panel">
        <div class="panel-heading">
          <div>
            <h2>Mount Table</h2>
            <p>Manage UFS mappings exposed through Curvine</p>
          </div>
          <button class="admin-button primary compact" @click="openAddMount">Add Mount</button>
        </div>

        <div class="mode-tabs" role="tablist" aria-label="Mount mode filter">
          <button :class="['mode-tab', { active: modeFilter === 'all' }]" @click="modeFilter = 'all'">All {{ mounts.length }}</button>
          <button :class="['mode-tab', { active: modeFilter === 'fs_mode' }]" @click="modeFilter = 'fs_mode'">FS Mode {{ fsModeCount }}</button>
          <button :class="['mode-tab', { active: modeFilter === 'cache_mode' }]" @click="modeFilter = 'cache_mode'">Cache Mode {{ cacheModeCount }}</button>
        </div>

        <div class="mount-table-scroll">
          <table class="admin-table mount-table">
          <thead>
            <tr><th>Curvine Path</th><th>UFS URI</th><th>Mode</th><th>TTL</th><th>Provider</th><th>Resync</th><th></th></tr>
          </thead>
          <tbody>
            <tr v-for="mount in filteredMounts" :key="mount.mount_id || mount.cv_path">
              <td><strong>{{ mount.cv_path }}</strong></td>
              <td class="ufs-cell">{{ mount.ufs_path }}</td>
              <td><span :class="['mode-pill', writeTypeLabel(mount.write_type)]">{{ writeTypeLabel(mount.write_type) }}</span></td>
              <td>{{ formatTtl(mount.ttl_ms) }}</td>
              <td>{{ providerLabel(mount.provider) || schemeOf(mount.ufs_path) || 'auto' }}</td>
              <td class="resync-cell">
                <button v-if="isFsMode(mount)" class="link-button" :disabled="isResyncRunning(mount)" @click="resyncMount(mount)">Resync</button>
                <span v-else class="muted-text">cache mode</span>
                <span v-if="resyncTasks[mount.cv_path]" class="resync-progress" :title="resyncLabel(resyncTasks[mount.cv_path])">{{ resyncLabel(resyncTasks[mount.cv_path]) }}</span>
              </td>
              <td>
                <div class="row-actions mount-actions">
                  <button class="link-button" @click="editMount(mount)">Edit</button>
                  <button class="link-button" @click="validateExisting(mount)">Validate</button>
                  <button class="link-button danger-link" @click="removeMount(mount)">Delete</button>
                </div>
              </td>
            </tr>
          </tbody>
          </table>
        </div>
        <div v-if="filteredMounts.length === 0" class="empty-state">No mounts in this mode.</div>
      </div>
    </section>

    <div v-if="showMountForm" class="admin-modal-backdrop" @click.self="cancelForm">
      <div class="admin-panel form-panel mount-form admin-modal-panel">
        <div class="panel-heading">
          <div>
            <h2>{{ form.update ? 'Update Mount' : 'Add Mount' }}</h2>
            <p>Configure or update a UFS path mounted into Curvine</p>
          </div>
          <button class="admin-button ghost compact" @click="cancelForm">Close</button>
        </div>

        <label>Curvine Path<input v-model="form.cv_path" placeholder="/lake/raw"></label>
        <label>UFS URI<input v-model="form.ufs_path" placeholder="file:///data/raw or s3://bucket/raw"></label>
        <div class="form-row">
          <label>Write Type
            <select
              v-model="form.write_type"
              :disabled="form.update"
              :title="form.update ? '更新 mount 不支持切换模式' : ''"
            >
              <option value="fs_mode">fs_mode</option>
              <option value="cache_mode">cache_mode</option>
            </select>
          </label>
          <label>TTL<input v-model="form.ttl" placeholder="7d"></label>
        </div>
        <div class="form-row">
          <label>Provider
            <select v-model="form.provider">
              <option value="auto">auto</option>
              <option value="opendal">opendal</option>
              <option value="oss-hdfs">oss-hdfs</option>
            </select>
          </label>
          <label>Replicas<input v-model.number="form.replicas" type="number" min="1" placeholder="default"></label>
        </div>
        <label>Block Size<input v-model="form.block_size" placeholder="128MB"></label>
        <label class="check-line"><input v-model="form.read_verify_ufs" type="checkbox"> Read verify UFS</label>
        <label>Properties<textarea v-model="propertiesText" rows="5" placeholder="key=value
access_key=..."></textarea></label>

        <div class="form-actions">
          <button class="admin-button ghost" @click="validateForm">Validate</button>
          <button class="admin-button primary" @click="submitForm">{{ form.update ? 'Update' : 'Mount' }}</button>
          <button class="admin-button ghost" @click="cancelForm">Cancel</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script>
import MetricCard from '@/components/admin/MetricCard.vue'
import { deleteMount, fetchMountResync, fetchMountsData, saveMount, startMountResync, validateMount } from '@/api/client'
import eventBus from '@/utils/eventBus'

const emptyForm = () => ({
  cv_path: '',
  ufs_path: '',
  update: false,
  write_type: 'fs_mode',
  ttl: '7d',
  read_verify_ufs: false,
  replicas: null,
  block_size: '',
  storage_type: '',
  provider: 'auto',
  properties: {}
})

export default {
  name: 'MountsPage',
  components: { MetricCard },
  data() {
    return {
      loading: false,
      mounts: [],
      modeFilter: 'all',
      form: emptyForm(),
      propertiesText: '',
      showMountForm: false,
      resyncTasks: {},
      resyncTimers: {}
    }
  },
  computed: {
    fsModeCount() { return this.mounts.filter((m) => this.isFsMode(m)).length },
    cacheModeCount() { return this.mounts.filter((m) => this.writeTypeLabel(m.write_type) === 'cache_mode').length },
    providerCount() { return new Set(this.mounts.map((m) => this.providerLabel(m.provider) || this.schemeOf(m.ufs_path)).filter(Boolean)).size },
    filteredMounts() {
      if (this.modeFilter === 'all') return this.mounts
      return this.mounts.filter((m) => this.writeTypeLabel(m.write_type) === this.modeFilter)
    }
  },
  created() { this.fetchData() },
  mounted() { eventBus.on('admin-refresh', this.fetchData) },
  beforeUnmount() {
    eventBus.off('admin-refresh', this.fetchData)
    Object.values(this.resyncTimers).forEach((timer) => window.clearTimeout(timer))
  },
  methods: {
    async fetchData() {
      this.loading = true
      try {
        const data = await fetchMountsData()
        this.mounts = Array.isArray(data) ? data : (data.items || [])
      } finally { this.loading = false }
    },
    parseProperties() {
      const props = {}
      for (const line of this.propertiesText.split('\n')) {
        const value = line.trim()
        if (!value) continue
        const index = value.indexOf('=')
        if (index <= 0) throw new Error(`Invalid property: ${value}`)
        props[value.slice(0, index).trim()] = value.slice(index + 1).trim()
      }
      return props
    },
    normalizeUfsPath(path) {
      const value = String(path || '').trim()
      return value.startsWith('/') ? `file://${value}` : value
    },
    ensureFormValid() {
      if (!String(this.form.cv_path || '').trim()) throw new Error('Curvine Path is required')
      if (!String(this.form.ufs_path || '').trim()) throw new Error('UFS URI is required')
    },
    payload() {
      this.ensureFormValid()
      return {
        ...this.form,
        update: Boolean(this.form.update),
        cv_path: String(this.form.cv_path || '').trim(),
        ufs_path: this.normalizeUfsPath(this.form.ufs_path),
        replicas: this.form.replicas || null,
        block_size: this.form.block_size || null,
        storage_type: this.form.storage_type || null,
        provider: this.form.provider === 'auto' ? null : this.form.provider,
        properties: this.parseProperties()
      }
    },
    async submitForm() {
      this.loading = true
      try {
        await saveMount(this.payload())
        window.alert(this.form.update ? 'Mount updated.' : 'Mount created.')
        this.closeForm()
        await this.fetchData()
      } catch (error) {
        window.alert(`Mount failed: ${error}`)
      } finally { this.loading = false }
    },
    async validateForm() {
      this.loading = true
      try {
        this.ensureFormValid()
        const data = await validateMount({ ufs_path: this.normalizeUfsPath(this.form.ufs_path), provider: this.form.provider || null, properties: this.parseProperties() })
        window.alert(`Valid UFS path. Entries: ${data.entries}`)
      } catch (error) {
        window.alert(`Validate failed: ${error}`)
      } finally { this.loading = false }
    },
    async validateExisting(mount) {
      this.loading = true
      try {
        const data = await validateMount({ ufs_path: mount.ufs_path, provider: mount.provider || null, properties: mount.properties || {} })
        window.alert(`Valid UFS path. Entries: ${data.entries}`)
      } catch (error) {
        window.alert(`Validate failed: ${error}`)
      } finally { this.loading = false }
    },
    async resyncMount(mount) {
      try {
        const task = await startMountResync(mount.cv_path)
        this.setResyncTask(mount.cv_path, task)
        this.pollResync(mount.cv_path, task.id)
      } catch (error) {
        window.alert(`Resync failed: ${error}`)
      }
    },
    async pollResync(cvPath, taskId) {
      try {
        const task = await fetchMountResync(taskId)
        this.setResyncTask(cvPath, task)
        if (!task.done) {
          this.resyncTimers[cvPath] = window.setTimeout(() => this.pollResync(cvPath, taskId), 1000)
        } else {
          await this.fetchData()
        }
      } catch (error) {
        this.setResyncTask(cvPath, { id: taskId, status: 'failed', done: true, message: String(error), scanned: 0, recreated: 0, skipped: 0, failed: 1, pending_dirs: 0 })
      }
    },
    setResyncTask(cvPath, task) {
      this.resyncTasks = { ...this.resyncTasks, [cvPath]: task }
    },
    isResyncRunning(mount) {
      const task = this.resyncTasks[mount.cv_path]
      return Boolean(task && !task.done)
    },
    resyncLabel(task) {
      const status = task.status || 'running'
      return `${status}: scanned ${task.scanned || 0}, recreated ${task.recreated || 0}, skipped ${task.skipped || 0}, failed ${task.failed || 0}, pending ${task.pending_dirs || 0}`
    },
    async removeMount(mount) {
      if (!window.confirm(`Delete mount ${mount.cv_path}?`)) return
      this.loading = true
      try {
        await deleteMount(mount.cv_path)
        window.alert(`Deleted mount: ${mount.cv_path}`)
        await this.fetchData()
      } catch (error) {
        window.alert(`Delete failed: ${error}`)
      } finally { this.loading = false }
    },
    openAddMount() {
      this.resetForm()
      this.showMountForm = true
    },
    editMount(mount) {
      this.form = {
        cv_path: mount.cv_path,
        ufs_path: mount.ufs_path,
        update: true,
        write_type: this.writeTypeLabel(mount.write_type),
        ttl: this.formatTtl(mount.ttl_ms),
        read_verify_ufs: Boolean(mount.read_verify_ufs),
        replicas: mount.replicas || null,
        block_size: mount.block_size ? String(mount.block_size) : '',
        storage_type: mount.storage_type || '',
        provider: this.providerLabel(mount.provider),
        properties: mount.properties || {}
      }
      this.propertiesText = Object.entries(mount.properties || {}).map(([k, v]) => `${k}=${v}`).join('\n')
      this.showMountForm = true
    },
    resetForm() { this.form = emptyForm(); this.propertiesText = '' },
    closeForm() { this.showMountForm = false; this.resetForm() },
    cancelForm() { this.closeForm() },
    isFsMode(mount) { return this.writeTypeLabel(mount.write_type) === 'fs_mode' },
    writeTypeLabel(value) {
      if (value === 1 || value === 'FsMode') return 'fs_mode'
      if (value === 0 || value === 'CacheMode') return 'cache_mode'
      return String(value || 'fs_mode').toLowerCase()
    },
    providerLabel(value) {
      if (!value) return 'auto'
      if (value === 'OssHdfs') return 'oss-hdfs'
      if (value === 'Opendal') return 'opendal'
      if (value === 'Auto') return 'auto'
      return String(value).toLowerCase()
    },
    schemeOf(path) { return String(path || '').split(':')[0] },
    formatTtl(ms) {
      const value = Number(ms || 0)
      if (!value) return '0ms'
      if (value % 86400000 === 0) return `${value / 86400000}d`
      if (value % 3600000 === 0) return `${value / 3600000}h`
      if (value % 60000 === 0) return `${value / 60000}m`
      if (value % 1000 === 0) return `${value / 1000}s`
      return `${value}ms`
    }
  }
}
</script>

<style scoped>
.mounts-grid {
  grid-template-columns: 1fr;
  align-items: start;
}

.mount-table-scroll {
  width: 100%;
  overflow: visible;
}

.mount-table {
  min-width: 0;
  table-layout: fixed;
}

.mount-table th,
.mount-table td {
  padding-left: clamp(4px, .55vw, 8px);
  padding-right: clamp(4px, .55vw, 8px);
  font-size: clamp(11px, .78vw, 13px);
}

.mount-table th:nth-child(1) { width: 15%; }
.mount-table th:nth-child(2) { width: 24%; }
.mount-table th:nth-child(3) { width: 14%; }
.mount-table th:nth-child(4) { width: 7%; }
.mount-table th:nth-child(5) { width: 8%; }
.mount-table th:nth-child(6) { width: 14%; }
.mount-table th:nth-child(7) { width: 18%; }

.mode-tabs {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 16px;
}

.mode-tab {
  border: 1px solid #cfe0c7;
  background: #fff;
  color: #243525;
  border-radius: 8px;
  padding: 8px clamp(8px, .75vw, 12px);
  font-size: clamp(12px, .9vw, 14px);
  font-weight: 700;
  cursor: pointer;
}

.mode-tab.active {
  background: #5ca63a;
  border-color: #5ca63a;
  color: #fff;
}

.mount-table td strong,
.mount-table td span,
.ufs-cell {
  white-space: normal;
  overflow-wrap: anywhere;
}

.ufs-cell {
  max-width: none;
  word-break: break-word;
}

.mode-pill {
  display: inline-flex;
  align-items: center;
  max-width: 100%;
  min-height: 22px;
  border-radius: 6px;
  padding: 2px 6px;
  font-size: clamp(10px, .72vw, 12px);
  font-weight: 800;
  background: #eef6ea;
  color: #2c6c1f;
  overflow-wrap: anywhere;
}

.mode-pill.cache_mode {
  background: #eef3f8;
  color: #315f7f;
}

.resync-cell {
  min-width: 0;
  max-width: none;
  white-space: normal;
}

.resync-progress {
  display: block;
  color: #51604c;
  font-size: clamp(10px, .72vw, 12px);
  line-height: 1.35;
  margin-top: 4px;
  max-width: 100%;
  white-space: normal;
  overflow: visible;
  text-overflow: clip;
  word-break: break-word;
  cursor: help;
}

.muted-text {
  color: #88967f;
  font-size: 12px;
}

.link-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.mount-form,
.mount-form label,
.mount-form input,
.mount-form select,
.mount-form textarea {
  min-width: 0;
}

.mount-form label {
  display: grid;
  grid-template-columns: 1fr;
  gap: 6px;
}

.mount-form input:not([type="checkbox"]),
.mount-form select,
.mount-form textarea {
  width: 100%;
  max-width: 100%;
  border: 1px solid #d9e4d2;
  border-radius: 8px;
  padding: 9px 10px;
  font: inherit;
  background: #fff;
}

.form-row {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.check-line {
  display: inline-flex !important;
  grid-template-columns: none !important;
  align-items: center;
  justify-content: flex-start;
  gap: 8px !important;
  width: fit-content;
  max-width: 100%;
  line-height: 1.2;
}

.check-line input[type="checkbox"] {
  flex: 0 0 auto;
  width: 16px;
  height: 16px;
  margin: 0;
  padding: 0;
  accent-color: var(--admin-primary);
}

.form-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.mount-table .mount-actions {
  align-items: center;
  gap: 3px 6px;
}

.mount-table .link-button {
  height: 26px;
  padding: 0 2px;
  font-size: clamp(10px, .72vw, 12px);
}

@media (max-width: 1180px) {
  .mounts-grid {
    grid-template-columns: minmax(0, 1.35fr) minmax(280px, .65fr);
    gap: 12px;
  }

  .admin-panel {
    padding: 14px;
  }

  .form-row { grid-template-columns: 1fr; }
}

@media (max-width: 860px) {
  .mounts-grid {
    grid-template-columns: 1fr;
  }
}


.admin-modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgba(18, 32, 20, .28);
}

.admin-modal-panel {
  width: min(720px, calc(100vw - 48px));
  max-height: calc(100vh - 48px);
  overflow: auto;
  box-shadow: 0 24px 70px rgba(31, 48, 26, .24);
}

</style>

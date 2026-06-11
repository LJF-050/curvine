<template>
  <div class="admin-page" v-loading="loading">
    <section class="admin-panel">
      <div class="panel-heading table-heading"><div><h2>Worker Detail</h2><p>{{ title }}</p></div><div class="row-actions"><button class="admin-button ghost compact" @click="$router.push('/workers')">Back</button><button v-if="manageable && statusName !== 'Blacklist'" class="admin-button ghost compact" @click="runAction('blacklist')">AddBlacklist</button><button v-if="statusName === 'Blacklist'" class="admin-button ghost compact" @click="runAction('allow')">Allow</button><button v-if="manageable && statusName !== 'Decommission'" class="admin-button ghost compact" @click="runAction('decommission')">Decommission</button><button v-if="statusName === 'Decommission'" class="admin-button ghost compact" @click="runAction('recommission')">Recommission</button></div></div>
      <div class="detail-summary">
        <div><span>Status</span><strong><span :class="['status-pill', statusName.toLowerCase() || 'unknown']">{{ statusName || '-' }}</span></strong></div>
        <div><span>Usage</span><strong>{{ usagePercent }}%</strong><div class="inline-meter"><span :style="{ width: usagePercent + '%' }"></span></div></div>
        <div><span>Capacity</span><strong>{{ formatBytes(worker.capacity) }}</strong></div>
        <div><span>Available</span><strong>{{ formatBytes(worker.available) }}</strong></div>
      </div>
    </section>

    <section class="content-grid">
      <div class="admin-panel">
        <div class="panel-heading"><div><h2>Runtime</h2><p>Address, heartbeat and reported counters</p></div></div>
        <dl class="detail-list">
          <div><dt>Hostname</dt><dd>{{ workerHostname(worker) }}</dd></div>
          <div><dt>Worker ID</dt><dd>{{ workerId(worker) }}</dd></div>
          <div><dt>RPC Address</dt><dd>{{ workerAddress(worker) }}</dd></div>
          <div><dt>Web Port</dt><dd>{{ address.web_port || '-' }}</dd></div>
          <div><dt>IP</dt><dd>{{ displayIp }}</dd></div>
          <div><dt>Heartbeat</dt><dd>{{ formatTime(worker.last_update) }}</dd></div>
          <div><dt>Version</dt><dd>{{ detail.version || 'Not reported' }}</dd></div>
          <div><dt>Files</dt><dd>{{ detail.file_count ?? 'Not reported' }}</dd></div>
          <div><dt>Blocks</dt><dd>{{ worker.block_num || 0 }}</dd></div>
        </dl>
      </div>

      <div class="admin-panel">
        <div class="panel-heading"><div><h2>Storage</h2><p>Aggregated storage reported by this worker</p></div></div>
        <dl class="detail-list">
          <div><dt>Curvine Used</dt><dd>{{ formatBytes(worker.fs_used) }}</dd></div>
          <div><dt>Non-FS Used</dt><dd>{{ formatBytes(worker.non_fs_used) }}</dd></div>
          <div><dt>Reserved</dt><dd>{{ formatBytes(worker.reserved_bytes) }}</dd></div>
          <div><dt>Storage Dirs</dt><dd>{{ storageItems.length }}</dd></div>
        </dl>
      </div>
    </section>

    <section class="admin-panel">
      <div class="panel-heading table-heading"><div><h2>Storage Directories</h2><p>Per-disk capacity and block distribution</p></div></div>
      <div class="storage-list">
        <article v-for="item in storageItems" :key="item.storage_id" class="storage-row">
          <div class="storage-main"><strong>{{ item.dir_path }}</strong><span>{{ item.storage_id }}</span></div>
          <div class="worker-meta"><span>Type</span><strong>{{ item.storage_type || '-' }}</strong></div>
          <div class="worker-usage"><div class="usage-line"><strong>{{ storageUsage(item) }}%</strong><span>{{ formatBytes(item.capacity - item.available) }} / {{ formatBytes(item.capacity) }}</span></div><div class="inline-meter"><span :style="{ width: storageUsage(item) + '%' }"></span></div><small>{{ formatBytes(item.available) }} available</small></div>
          <div class="worker-meta"><span>Blocks</span><strong>{{ item.block_num || 0 }}</strong></div>
          <div class="worker-state"><span :class="['status-pill', item.failed ? 'failed' : 'healthy']">{{ item.failed ? 'failed' : 'healthy' }}</span></div>
        </article>
      </div>
      <div v-if="storageItems.length === 0" class="empty-state">No storage directories reported.</div>
    </section>
  </div>
</template>

<script>
import { fetchWorkerDetail, updateWorkerAction } from '@/api/client'
import { formatBytes, formatTime, workerAddress, workerHostname, workerId } from '@/utils/format'

export default {
  name: 'WorkerDetailPage',
  data() { return { loading: false, detail: {}, worker: {} } },
  computed: {
    address() { return this.worker.address || {} },
    title() { return this.workerHostname(this.worker) !== '-' ? `${this.workerHostname(this.worker)} - ${this.workerAddress(this.worker)}${this.displayIp !== '-' ? ` - IP ${this.displayIp}` : ''}` : 'Worker status and storage details' },
    statusName() { return this.detail.state || this.worker.status || '' },
    manageable() { return this.statusName !== 'Lost' },
    usagePercent() { return this.workerUsage(this.worker) },
    storageItems() { return Object.values(this.worker.storage_map || {}) },
    displayIp() { return this.worker.display_ip || this.address.ip_addr || '-' }
  },
  created() { this.fetchData() },
  methods: {
    formatBytes, formatTime, workerAddress, workerHostname, workerId,
    async fetchData() {
      this.loading = true
      try {
        this.detail = await fetchWorkerDetail(this.$route.params.id)
        this.worker = this.detail.worker || {}
      } finally { this.loading = false }
    },
    workerSelector(worker) { return this.workerAddress(worker) },
    workerUsage(worker) { const cap = Number(worker.capacity || 0); const avail = Number(worker.available || 0); return cap ? Math.max(0, Math.min(100, Math.round(((cap - avail) / cap) * 100))) : 0 },
    storageUsage(item) { const cap = Number(item.capacity || 0); const avail = Number(item.available || 0); return cap ? Math.max(0, Math.min(100, Math.round(((cap - avail) / cap) * 100))) : 0 },
    async runAction(action) {
      const selector = this.workerSelector(this.worker)
      if (!window.confirm(`${action} worker ${selector}?`)) return
      this.loading = true
      try {
        await updateWorkerAction(selector, action)
        await this.fetchData()
      } catch (error) {
        window.alert(`Worker action failed: ${error}`)
      } finally { this.loading = false }
    }
  }
}
</script>

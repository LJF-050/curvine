<template>
  <div class="admin-page" v-loading="loading">
    <section class="metric-grid">
      <MetricCard label="Live Workers" :value="liveWorkers.length" meta="healthy heartbeat" delta="online" tone="good" />
      <MetricCard label="Managed" :value="managedWorkers.length" meta="blacklist / decommission" :delta="`${blacklistWorkers.length} / ${decommissionWorkers.length}`" :tone="managedWorkers.length ? 'warn' : 'neutral'" />
      <MetricCard label="Total Capacity" :value="formatBytes(totalCapacity)" meta="active workers" :delta="`${usagePercent}% used`" tone="neutral" />
      <MetricCard label="Lost Workers" :value="lostWorkers.length" meta="requires attention" :delta="lostWorkers.length ? 'check' : 'none'" :tone="lostWorkers.length ? 'bad' : 'good'" />
    </section>

    <section class="admin-panel">
      <div class="panel-heading table-heading"><div><h2>Worker Nodes</h2><p>Worker status, capacity, heartbeat and management actions</p></div><div class="segmented"><button :class="{ active: statusFilter === 'all' }" @click="statusFilter = 'all'">All</button><button :class="{ active: statusFilter === 'live' }" @click="statusFilter = 'live'">Live</button><button :class="{ active: statusFilter === 'blacklist' }" @click="statusFilter = 'blacklist'">Blacklist</button><button :class="{ active: statusFilter === 'decommission' }" @click="statusFilter = 'decommission'">Decommission</button><button :class="{ active: statusFilter === 'lost' }" @click="statusFilter = 'lost'">Lost</button></div></div>
      <div class="toolbar"><input v-model="query" class="admin-input" placeholder="Search hostname or ip"></div>

      <div class="worker-list">
        <div class="worker-row worker-header"><span>Worker</span><span>Status</span><span>Usage</span><span>Blocks</span><span>Heartbeat</span><span>Actions</span></div>
        <article v-for="worker in filteredWorkers" :key="workerSelector(worker)" class="worker-row">
          <div class="worker-main file-name-cell" @click="openDetail(worker)">
            <strong>{{ workerHostname(worker) }}</strong>
            <small>RPC {{ rpcPort(worker) }}<template v-if="displayIp(worker)"> · IP {{ displayIp(worker) }}</template></small>
          </div>
          <div class="worker-state"><span :class="['status-pill', worker._status]">{{ worker._status }}</span></div>
          <div class="worker-usage">
            <div class="usage-line"><strong>{{ workerUsage(worker) }}%</strong><span>{{ formatBytes(usedBytes(worker)) }} / {{ formatBytes(worker.capacity) }}</span></div>
            <div class="inline-meter"><span :style="{ width: workerUsage(worker) + '%' }"></span></div>
            <small>{{ formatBytes(worker.available) }} available</small>
          </div>
          <div class="worker-meta"><span>Blocks</span><strong>{{ worker.block_num || 0 }}</strong></div>
          <div class="worker-meta"><span>Heartbeat</span><strong>{{ formatHeartbeat(worker) }}</strong></div>
          <div class="row-actions worker-actions"><button class="link-button" @click="openDetail(worker)">Detail</button><button v-if="worker._status !== 'lost' && worker._status !== 'blacklist'" class="link-button" @click="runAction(worker, 'blacklist')">AddBlacklist</button><button v-if="worker._status === 'blacklist'" class="link-button" @click="runAction(worker, 'allow')">Allow</button><button v-if="worker._status !== 'lost' && worker._status !== 'decommission'" class="link-button" @click="runAction(worker, 'decommission')">Decommission</button><button v-if="worker._status === 'decommission'" class="link-button" @click="runAction(worker, 'recommission')">Recommission</button></div>
        </article>
      </div>
      <div v-if="filteredWorkers.length === 0" class="empty-state">No workers matched.</div>
    </section>
  </div>
</template>

<script>
import MetricCard from '@/components/admin/MetricCard.vue'
import { fetchWorkersData, updateWorkerAction } from '@/api/client'
import { formatBytes, formatTime, workerAddress, workerHostname, workerId } from '@/utils/format'
import eventBus from '@/utils/eventBus'

export default {
  name: 'WorkersPage',
  components: { MetricCard },
  data() { return { loading: false, data: {}, statusFilter: 'all', query: '' } },
  computed: {
    liveWorkers() { return (this.data.live_workers || []).map((w) => ({ ...w, _status: 'live' })) },
    blacklistWorkers() { return (this.data.blacklist_workers || []).map((w) => ({ ...w, _status: 'blacklist' })) },
    decommissionWorkers() { return (this.data.decommission_workers || []).map((w) => ({ ...w, _status: 'decommission' })) },
    lostWorkers() { return (this.data.lost_workers || []).map((w) => ({ ...w, _status: 'lost' })) },
    managedWorkers() { return [...this.blacklistWorkers, ...this.decommissionWorkers] },
    workers() { return [...this.liveWorkers, ...this.blacklistWorkers, ...this.decommissionWorkers, ...this.lostWorkers] },
    filteredWorkers() { const q = this.query.toLowerCase(); return this.workers.filter((w) => (this.statusFilter === 'all' || w._status === this.statusFilter) && `${workerHostname(w)} ${workerId(w)} ${workerAddress(w)} ${this.ipAddress(w)}`.toLowerCase().includes(q)) },
    activeWorkers() { return [...this.liveWorkers, ...this.blacklistWorkers, ...this.decommissionWorkers] },
    totalCapacity() { return this.activeWorkers.reduce((sum, w) => sum + Number(w.capacity || 0), 0) },
    totalAvailable() { return this.activeWorkers.reduce((sum, w) => sum + Number(w.available || 0), 0) },
    usagePercent() { return this.totalCapacity ? Math.round(((this.totalCapacity - this.totalAvailable) / this.totalCapacity) * 100) : 0 }
  },
  created() { this.fetchData() },
  mounted() { eventBus.on('admin-refresh', this.fetchData) },
  beforeUnmount() { eventBus.off('admin-refresh', this.fetchData) },
  methods: {
    formatBytes, workerAddress, workerHostname, workerId,
    async fetchData() { this.loading = true; try { this.data = await fetchWorkersData() || {} } finally { this.loading = false } },
    ipAddress(worker) { return worker.display_ip || (worker.address && worker.address.ip_addr) || worker.ip_addr || '-' },
    workerHost(worker) { return (worker.address && worker.address.hostname) || worker.hostname || '-' },
    rpcPort(worker) { return (worker.address && worker.address.rpc_port) || worker.rpc_port || '-' },
    displayIp(worker) { const ip = this.ipAddress(worker); return ip && ip !== '-' ? ip : '' },
    usedBytes(worker) { return Math.max(0, Number(worker.capacity || 0) - Number(worker.available || 0)) },
    workerUsage(worker) { const cap = Number(worker.capacity || 0); return cap ? Math.max(0, Math.min(100, Math.round((this.usedBytes(worker) / cap) * 100))) : 0 },
    formatHeartbeat(worker) { return formatTime(worker.last_update_ms || worker.last_update) },
    workerSelector(worker) { return workerAddress(worker) },
    openDetail(worker) { this.$router.push({ path: `/workers/${encodeURIComponent(this.workerSelector(worker))}` }) },
    async runAction(worker, action) {
      const labels = { blacklist: 'blacklist', allow: 'allow', decommission: 'decommission', recommission: 'recommission' }
      if (!window.confirm(`${labels[action]} worker ${this.workerSelector(worker)}?`)) return
      this.loading = true
      try {
        await updateWorkerAction(this.workerSelector(worker), action)
        await this.fetchData()
      } catch (error) {
        window.alert(`Worker action failed: ${error}`)
      } finally { this.loading = false }
    }
  }
}
</script>

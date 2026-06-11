<template>
  <div class="admin-page" v-loading="loading">
    <section class="metric-grid">
      <MetricCard label="Master" :value="masterStateLabel" :meta="activeMasterDisplay" :delta="overview.cluster_id || '-'" :tone="masterStateLabel === 'Active' ? 'good' : (masterStateLabel === 'Standby' ? 'warn' : 'neutral')" />
      <MetricCard label="HA Nodes" :value="masterNodes.length || '-'" :meta="standbyLabel" :delta="masterHa.failover_supported ? 'switchable' : 'readonly'" :tone="masterNodes.length > 1 ? 'good' : 'neutral'" />
      <MetricCard label="Workers" :value="workerTotal" :meta="`${overview.live_workers || 0} live, ${overview.lost_workers || 0} lost`" :delta="overview.lost_workers ? 'attention' : 'healthy'" :tone="overview.lost_workers ? 'warn' : 'good'" />
      <MetricCard label="Capacity" :value="formatBytes(overview.capacity)" :meta="`${formatBytes(overview.available)} available`" :delta="`${capacityUsedLabel} used`" tone="neutral" />
      <MetricCard label="Namespace" :value="formatCompactNumber(namespaceTotal)" :meta="`${formatCompactNumber(overview.files_total)} files`" :delta="`${formatCompactNumber(overview.dir_total)} dirs`" tone="good" />
    </section>

    <section class="content-grid">
      <div class="admin-panel">
        <div class="panel-heading"><div><h2>Cluster Capacity</h2><p>Storage usage reported by Master</p></div></div>
        <div class="capacity-layout">
          <div class="capacity-donut" :style="capacityStyle"><div><strong>{{ capacityUsedLabel }}</strong><span>Used</span></div></div>
          <div class="capacity-list">
            <div class="capacity-row"><div><span>Filesystem Used</span><strong>{{ formatBytes(overview.fs_used) }}</strong></div><div class="meter"><span :style="{ width: capacityUsedPercent + '%' }"></span></div></div>
            <div class="capacity-row"><div><span>Available</span><strong>{{ formatBytes(overview.available) }}</strong></div><div class="meter"><span :style="{ width: availablePercent + '%' }"></span></div></div>
            <div class="capacity-row"><div><span>Reserved</span><strong>{{ formatBytes(overview.reserved_bytes) }}</strong></div><div class="meter"><span :style="{ width: reservedPercent + '%' }"></span></div></div>
          </div>
        </div>
      </div>

      <div class="admin-panel">
        <div class="panel-heading"><div><h2>Master Details</h2><p>Runtime identity and startup state</p></div></div>
        <dl class="detail-list">
          <div><dt>Cluster ID</dt><dd>{{ overview.cluster_id || '-' }}</dd></div>
          <div><dt>Configured Master</dt><dd>{{ configuredMasterDisplay }}</dd></div>
          <div><dt>Active Master</dt><dd>{{ activeMasterDisplayDetail || '-' }}</dd></div>
          <div><dt>Start Time</dt><dd>{{ overview.start_time || '-' }}</dd></div>
          <div><dt>Block Total</dt><dd>{{ formatNumber(overview.block_total) }}</dd></div>
        </dl>
      </div>
    </section>

    <section class="admin-panel table-panel">
      <div class="panel-heading table-heading">
        <div><h2>Master HA</h2><p>Active and standby masters discovered from cluster configuration</p></div>
        <button class="admin-button ghost compact" :disabled="haLoading" @click="fetchHa">Refresh</button>
      </div>
      <table v-if="masterNodes.length" class="admin-table master-ha-table">
        <thead>
          <tr><th>Master</th><th>Journal</th><th>Role</th><th>Reachable</th><th>Current</th><th>Action</th></tr>
        </thead>
        <tbody>
          <tr v-for="node in masterNodes" :key="node.addr">
            <td><strong>{{ node.display_addr || node.addr }}</strong></td>
            <td><span>{{ node.journal_addr || '-' }}</span></td>
            <td><span :class="['status-pill', roleTone(node.role)]">{{ node.role || 'Unknown' }}</span></td>
            <td>
              <span
                :class="['status-pill', node.reachable ? 'healthy' : 'failed']"
                :title="node.reachable_error || ''"
              >
                {{ node.reachable ? 'Reachable' : 'Unreachable' }}
              </span>
            </td>
            <td><span>{{ node.current ? 'Active target' : '-' }}</span></td>
            <td>
              <button
                class="admin-button ghost compact"
                :disabled="Boolean(switchingMaster) || !node.switchable"
                :title="switchTitle(node)"
                @click="switchMaster(node)"
              >
                {{ switchingMaster === node.addr ? 'Switching' : 'Switch' }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      <div v-else class="empty-state">No HA master nodes reported yet.</div>
      <div class="ha-note">
        <strong>Failover API:</strong>
        <span>{{ masterHa.failover_supported ? 'enabled' : 'unavailable' }}</span>
      </div>
    </section>
  </div>
</template>

<script>
import MetricCard from '@/components/admin/MetricCard.vue'
import { fetchOverviewData, fetchMasterHa, requestMasterFailover } from '@/api/client'
import { displayMasterAddr, formatBytes, formatNumber, formatCompactNumber, isLoopbackHost, normalizeCount } from '@/utils/format'
import eventBus from '@/utils/eventBus'

export default {
  name: 'OverviewPage',
  components: { MetricCard },
  data() { return { loading: false, haLoading: false, switchingMaster: '', overview: {}, masterHa: {} } },
  computed: {
    masterNodes() { return Array.isArray(this.masterHa.nodes) ? this.masterHa.nodes : [] },
    activeMaster() { return this.masterHa.active_master || this.overview.active_master || '' },
    activeMasterDisplay() { return this.activeMasterDisplayDetail || this.masterHa.current_master || this.overview.master_addr || '-' },
    configuredMasterDisplay() {
      const raw = this.masterHa.configured_master || this.overview.master_addr || '-'
      return displayMasterAddr(raw, this.preferredMasterHostname)
    },
    activeMasterDisplayDetail() {
      const raw = this.overview.active_master_display
        || this.masterHa.active_master_display
        || this.activeMaster
        || ''
      return displayMasterAddr(raw, this.preferredMasterHostname)
    },
    preferredMasterHostname() {
      const candidates = [
        this.overview.local_hostname,
        this.masterHa.local_hostname,
        this.hostnameFromAddr(this.activeMaster),
        this.hostnameFromHaNodes()
      ]
      return candidates.find((host) => host && !isLoopbackHost(host)) || ''
    },
    masterStateLabel() {
      const state = this.overview.master_state || this.masterHa.master_state
      if (state === 'Active' || state === 'Standby') return state
      if (!this.activeMaster) return 'Unknown'
      return this.activeMaster === (this.masterHa.current_master || this.overview.master_addr || '') ? 'Active' : 'Standby'
    },
    standbyLabel() {
      const standby = this.masterNodes.filter((node) => node.role === 'Standby').length
      return this.masterNodes.length ? `${standby} standby, ${this.activeMasterDisplay} active` : 'not configured'
    },
    workerTotal() { return (this.overview.live_workers || 0) + (this.overview.lost_workers || 0) },
    namespaceTotal() { return normalizeCount(this.overview.files_total) + normalizeCount(this.overview.dir_total) },
    capacityUsedPercent() { return this.percent(this.overview.fs_used, this.overview.capacity) },
    capacityUsedLabel() { return this.percentLabel(this.overview.fs_used, this.overview.capacity) },
    availablePercent() { return this.percent(this.overview.available, this.overview.capacity) },
    reservedPercent() { return this.percent(this.overview.reserved_bytes, this.overview.capacity) },
    capacityStyle() { return { background: `conic-gradient(#6fae3f 0 ${this.capacityUsedPercent}%, #e9f0e4 ${this.capacityUsedPercent}% 100%)` } }
  },
  created() { this.fetchData() },
  mounted() { eventBus.on('admin-refresh', this.fetchData) },
  beforeUnmount() { eventBus.off('admin-refresh', this.fetchData) },
  methods: {
    formatBytes, formatNumber, formatCompactNumber,
    hostnameFromAddr(addr) {
      const match = String(addr || '').match(/^([^:]+):/)
      return match ? match[1] : ''
    },
    hostnameFromHaNodes() {
      for (const node of this.masterNodes) {
        const host = this.hostnameFromAddr(node.display_addr || node.addr)
        if (host && !isLoopbackHost(host)) return host
      }
      return ''
    },
    percent(value, total) { return total ? Math.min(100, Math.round((Number(value || 0) / Number(total)) * 100)) : 0 },
    percentLabel(value, total) {
      if (!total || !value) return '0%'
      const percent = (Number(value) / Number(total)) * 100
      if (percent > 0 && percent < 0.01) return '<0.01%'
      if (percent < 1) return `${percent.toFixed(2)}%`
      return `${Math.min(100, Math.round(percent))}%`
    },
    roleTone(role) {
      if (role === 'Active') return 'healthy'
      if (role === 'Standby') return 'waiting'
      return 'neutral'
    },
    switchTitle(node) {
      if (!this.masterHa.failover_supported) return 'Failover API is unavailable'
      if (!node) return ''
      if (this.switchingMaster && this.switchingMaster !== node.addr) return `Switching to ${this.switchingMaster}`
      if (node.current || node.role === 'Active') return 'This master is already active'
      if (!node.reachable) return node.reachable_error || 'Target master is unreachable'
      return ''
    },
    async fetchHa() {
      this.haLoading = true
      try { this.masterHa = await fetchMasterHa() || {} } finally { this.haLoading = false }
    },
    async fetchData() {
      this.loading = true
      try {
        const [overviewResult, haResult] = await Promise.allSettled([fetchOverviewData(), fetchMasterHa()])
        if (overviewResult.status === 'fulfilled') this.overview = overviewResult.value || {}
        if (haResult.status === 'fulfilled') this.masterHa = haResult.value || {}
      } finally { this.loading = false }
    },
    async waitForActiveMaster(targetMaster) {
      let lastError = ''
      for (let index = 0; index < 30; index += 1) {
        await new Promise((resolve) => setTimeout(resolve, 1000))
        try {
          await this.fetchHa()
          if (this.masterHa.active_master === targetMaster) return
        } catch (err) {
          lastError = String(err)
        }
      }
      const suffix = lastError ? ` Last error: ${lastError}` : ''
      throw new Error(`Master switch request was accepted, but ${targetMaster} did not become active before timeout.${suffix}`)
    },
    async switchMaster(node) {
      if (!node || !node.addr) return
      const ok = window.confirm(`Switch active master from ${this.activeMasterDisplay} to ${node.addr}?`)
      if (!ok) return
      this.switchingMaster = node.addr
      try {
        await requestMasterFailover(node.addr)
        await this.waitForActiveMaster(node.addr)
        await this.fetchData()
      } catch (err) {
        window.alert(String(err))
      } finally {
        this.switchingMaster = ''
      }
    }
  }
}
</script>

<style scoped>
.master-ha-table th:nth-child(1) { width: 28%; }
.master-ha-table th:nth-child(2) { width: 24%; }
.master-ha-table th:nth-child(3),
.master-ha-table th:nth-child(4),
.master-ha-table th:nth-child(5) { width: 14%; }
.master-ha-table th:nth-child(6) { width: 10%; }
.ha-note { display: flex; gap: 8px; align-items: center; margin-top: 12px; color: var(--admin-muted); font-size: 13px; }
.ha-note strong { color: var(--admin-text); }
@media (max-width: 760px) { .ha-note { display: grid; } }
</style>

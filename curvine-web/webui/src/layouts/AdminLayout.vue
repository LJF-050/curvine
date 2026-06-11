<template>
  <div class="admin-shell">
    <aside class="admin-sidebar">
      <router-link class="admin-brand" to="/overview">
        <img src="@/assets/logo.svg" alt="Curvine">
        <div><strong>Curvine</strong><span>Admin Console</span></div>
      </router-link>
      <nav class="admin-nav">
        <router-link v-for="item in navItems" :key="item.path" :to="item.path" class="admin-nav-link">
          <span>{{ item.icon }}</span>{{ item.label }}
        </router-link>
      </nav>
      <div class="admin-sidebar-status"><span></span><div><strong>Master Service</strong><small>Ready for refresh</small></div></div>
    </aside>
    <main class="admin-main">
      <header class="admin-topbar">
        <div><h1>{{ currentTitle }}</h1><p>{{ currentSubtitle }}</p></div>
        <div class="admin-topbar-actions">
          <button class="admin-button ghost" type="button" @click="refreshPage">Refresh</button>
          <label class="admin-toggle"><input v-model="autoRefresh" type="checkbox">Auto refresh</label>
          <button class="admin-button ghost" type="button" @click="logoutPage">Logout</button>
        </div>
      </header>
      <router-view />
    </main>
  </div>
</template>

<script>
import eventBus from '@/utils/eventBus'
import { logout } from '@/api/client'

export default {
  name: 'AdminLayout',
  data() {
    return {
      autoRefresh: false,
      timer: null,
      navItems: [
        { path: '/overview', label: 'Overview', icon: 'O', subtitle: 'Cluster health, capacity and activity' },
        { path: '/browse', label: 'File System', icon: 'F', subtitle: 'Browse paths, inspect status and block placement' },
        { path: '/workers', label: 'Workers', icon: 'W', subtitle: 'Worker state, heartbeat and capacity usage' },
        { path: '/mounts', label: 'Mounts', icon: 'M', subtitle: 'UFS mappings and validation state' },
        { path: '/jobs', label: 'Load / Export Jobs', icon: 'J', subtitle: 'Manual load and fs_mode auto-export diagnostics' },
        { path: '/config', label: 'Config', icon: 'C', subtitle: 'Readonly cluster configuration' }
      ]
    }
  },
  computed: {
    currentItem() {
      return this.navItems.find((item) => this.$route.path.startsWith(item.path)) || this.navItems[0]
    },
    currentTitle() { return this.currentItem.label },
    currentSubtitle() { return this.currentItem.subtitle }
  },
  watch: {
    autoRefresh(value) {
      if (value) this.startAutoRefresh()
      else this.stopAutoRefresh()
    }
  },
  beforeUnmount() { this.stopAutoRefresh() },
  methods: {
    refreshPage() { eventBus.emit('admin-refresh') },
    async logoutPage() { await logout(); this.$router.replace('/login') },
    startAutoRefresh() { this.stopAutoRefresh(); this.timer = setInterval(this.refreshPage, 5000) },
    stopAutoRefresh() { if (this.timer) clearInterval(this.timer); this.timer = null }
  }
}
</script>

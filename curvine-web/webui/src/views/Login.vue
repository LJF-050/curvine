<template>
  <main class="login-page">
    <section class="login-panel">
      <div class="login-brand">
        <img src="@/assets/logo.svg" alt="Curvine">
        <div>
          <h1>Curvine</h1>
          <p>Admin Console</p>
        </div>
      </div>

      <form class="login-form" @submit.prevent="submit">
        <label>Username<input v-model="username" autocomplete="username" placeholder="Admin username" autofocus></label>
        <label>Password<input v-model="password" type="password" autocomplete="current-password" placeholder="Password"></label>
        <button class="admin-button primary" type="submit" :disabled="loading">{{ loading ? 'Signing in...' : 'Sign in' }}</button>
        <p v-if="error" class="login-error">{{ error }}</p>
        <p class="login-hint">Use the account configured for this Curvine deployment.</p>
      </form>
    </section>
  </main>
</template>

<script>
import { login } from '@/api/client'

export default {
  name: 'LoginPage',
  data() {
    return {
      username: '',
      password: '',
      loading: false,
      error: ''
    }
  },
  methods: {
    async submit() {
      this.error = ''
      if (!this.username || !this.password) {
        this.error = 'Username and password are required'
        return
      }
      this.loading = true
      try {
        await login({ username: this.username, password: this.password })
        const redirect = this.$route.query.redirect || '/overview'
        this.$router.replace(String(redirect))
      } catch (error) {
        this.error = String(error)
      } finally { this.loading = false }
    }
  }
}
</script>

<style scoped>
.login-page {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 24px;
  background: var(--admin-bg);
}

.login-panel {
  width: min(420px, 100%);
  border: 1px solid var(--admin-border);
  border-radius: 8px;
  background: #fff;
  padding: 28px;
}

.login-brand {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 24px;
}

.login-brand img { width: 40px; height: 40px; }
.login-brand h1 { margin: 0; font-size: 24px; line-height: 1.1; }
.login-brand p { margin: 4px 0 0; color: var(--admin-muted); }

.login-form,
.login-form label {
  display: grid;
  gap: 8px;
}

.login-form { gap: 14px; }
.login-form label { color: var(--admin-muted); font-size: 13px; font-weight: 700; }

.login-form input {
  width: 100%;
  height: 40px;
  border: 1px solid var(--admin-border-strong);
  border-radius: 8px;
  padding: 0 11px;
  color: var(--admin-text);
}

.login-error,
.login-hint {
  margin: 0;
  font-size: 13px;
}

.login-error { color: var(--admin-danger); }
.login-hint { color: var(--admin-muted); }
</style>

/*
 * Copyright 2025 OPPO.
 *
 * Licensed under the Apache License, Version 2.0.
 */

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import loadingDirective from '@/utils/loading'
import '@/styles/index.scss'

const app = createApp(App)

app.use(createPinia())
app.use(router)
app.directive('loading', loadingDirective)
app.mount('#app')

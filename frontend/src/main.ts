import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { Quasar, Notify, Dialog, Loading } from 'quasar'

// Quasar 图标与样式
import '@quasar/extras/material-icons/material-icons.css'
import 'quasar/src/css/index.sass'

// 自托管字体（@fontsource：chunked woff2，按 unicode-range 懒加载，无外部 CDN）
import '@fontsource/noto-sans-sc/400.css'
import '@fontsource/noto-sans-sc/500.css'
import '@fontsource/noto-sans-sc/700.css'
import '@fontsource/noto-serif-sc/600.css'
import '@fontsource/noto-serif-sc/700.css'
import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/500.css'
import '@fontsource/jetbrains-mono/700.css'

// PRTS 设计系统（覆盖 Quasar 暗色）
import './styles/theme.scss'

import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import { useAuthStore } from './stores/auth'

const app = createApp(App)

app.use(createPinia())
app.use(router)
app.use(i18n)
app.use(Quasar, {
  plugins: { Notify, Dialog, Loading },
  config: {
    // 默认深色（终端风）；用户切换后记忆于 localStorage
    dark: localStorage.getItem('prts_theme') !== 'light',
    notify: { position: 'top-right' },
  },
})

// 启动时从 localStorage 恢复会话（路由守卫会 await 同一 promise）
useAuthStore().ensureReady()

app.mount('#app')

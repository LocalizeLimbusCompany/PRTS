import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { quasar, transformAssetUrls } from '@quasar/vite-plugin'

const projectRoot = fileURLToPath(new URL('.', import.meta.url))

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [
    vue({ template: { transformAssetUrls } }),
    // Quasar 品牌色变量见 src/quasar-variables.sass
    quasar({ sassVariables: 'src/quasar-variables.sass' }),
  ],
  css: {
    preprocessorOptions: {
      // 让 Quasar 的 `@import 'src/quasar-variables.sass'` 能从项目根解析到变量文件。
      sass: { loadPaths: [projectRoot] },
      scss: { loadPaths: [projectRoot] },
    },
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 8080,
    // 开发期把 /api 与 /ws 代理到后端，避免跨域。
    // 后端路由位于根路径（如 /version），故转发时剥离 /api 前缀（与 nginx 生产配置一致）。
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/api/, ''),
      },
      '/ws': { target: 'ws://localhost:3000', ws: true },
    },
  },
  build: {
    target: 'esnext',
    outDir: 'dist',
  },
})

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getVersion } from '@/api'

/** 全局应用状态：后端连通性与版本。 */
export const useAppStore = defineStore('app', () => {
  /** 后端是否在线：null 表示尚未检测。 */
  const online = ref<boolean | null>(null)
  const version = ref<string>('')
  const loading = ref(false)

  /** 探测后端（调用 /version）。 */
  async function checkBackend() {
    loading.value = true
    try {
      const info = await getVersion()
      version.value = info.version
      online.value = true
    } catch {
      online.value = false
      version.value = ''
    } finally {
      loading.value = false
    }
  }

  return { online, version, loading, checkBackend }
})

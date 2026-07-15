import { computed, ref } from 'vue'

import {
  getUploadConfig,
  uploadsApi,
  type UploadBatchDto,
  type UploadBatchFileDto,
} from '@/api/uploads'

export interface UploadQueueFile {
  file: File
  path: string
  loaded: number
  state: 'waiting' | 'uploading' | 'uploaded' | 'failed'
  error: unknown | null
}

export const UPLOAD_BATCH_TERMINAL_STATES = new Set([
  'cancelled',
  'partially_succeeded',
  'succeeded',
  'failed',
  'expired',
])

/** Browser-side scheduler; file bytes are never parsed or copied into JSON. */
export function useUploadBatch(projectId: () => number) {
  const batch = ref<UploadBatchDto | null>(null)
  const queue = ref<UploadQueueFile[]>([])
  const running = ref(false)
  const cancelling = ref(false)
  const cancelRequested = ref(false)
  const controllers = new Map<number, AbortController>()
  const totalLoaded = computed(() => queue.value.reduce((sum, item) => sum + item.loaded, 0))
  const totalBytes = computed(() => queue.value.reduce((sum, item) => sum + item.file.size, 0))

  async function start(files: File[], destinationPath: string | null = null) {
    cancelRequested.value = false
    const config = await getUploadConfig()
    if (files.length === 0 || files.length > config.max_files_per_batch) {
      throw new Error('upload_file_count_exceeded')
    }
    const declared = files.map((file) => ({
      path: destinationPath ? `${destinationPath}/${file.name}` : file.name,
      size: file.size,
    }))
    if (declared.some((file) => file.size > config.max_bytes_per_file)) {
      throw new Error('upload_file_size_exceeded')
    }
    if (declared.reduce((sum, file) => sum + file.size, 0) > config.max_bytes_per_batch) {
      throw new Error('upload_batch_size_exceeded')
    }
    batch.value = await uploadsApi.createBatch(projectId(), declared)
    queue.value = files.map((file, index) => ({
      file,
      path: declared[index]!.path,
      loaded: 0,
      state: 'waiting',
      error: null,
    }))
    running.value = true
    try {
      let nextIndex = 0
      const worker = async () => {
        while (!cancelRequested.value && nextIndex < queue.value.length) {
          const index = nextIndex++
          const item = queue.value[index]!
          const remote = batch.value!.files[index]!
          const attempt = remote.attempts[0]!
          item.state = 'uploading'
          const controller = new AbortController()
          controllers.set(remote.id, controller)
          try {
            await uploadsApi.receiveAttempt(
              projectId(),
              batch.value!.id,
              remote.id,
              attempt.id,
              item.file,
              (loaded) => (item.loaded = loaded),
              controller.signal,
            )
            item.loaded = item.file.size
            item.state = 'uploaded'
          } catch (error) {
            item.state = 'failed'
            item.error = error
          } finally {
            controllers.delete(remote.id)
          }
        }
      }
      await Promise.all(
        Array.from({ length: Math.min(config.client_concurrency, files.length) }, () => worker()),
      )
      if (!cancelRequested.value && queue.value.some((item) => item.state === 'uploaded')) {
        batch.value = await uploadsApi.complete(projectId(), batch.value.id)
      } else {
        batch.value = await uploadsApi.get(projectId(), batch.value.id)
      }
      return batch.value
    } finally {
      running.value = false
    }
  }

  async function refresh() {
    if (!batch.value) return null
    batch.value = await uploadsApi.get(projectId(), batch.value.id)
    return batch.value
  }

  async function retry(remote: UploadBatchFileDto) {
    if (!batch.value || running.value) return
    const item = queue.value[remote.ordinal]
    if (!item || item.path !== remote.path) throw new Error('upload_source_file_unavailable')
    running.value = true
    item.loaded = 0
    item.error = null
    item.state = 'uploading'
    try {
      const attempt = await uploadsApi.retry(projectId(), batch.value.id, remote.id)
      const controller = new AbortController()
      controllers.set(remote.id, controller)
      try {
        await uploadsApi.receiveAttempt(
          projectId(),
          batch.value.id,
          remote.id,
          attempt.id,
          item.file,
          (loaded) => (item.loaded = loaded),
          controller.signal,
        )
      } finally {
        controllers.delete(remote.id)
      }
      item.loaded = item.file.size
      item.state = 'uploaded'
      batch.value = await uploadsApi.complete(projectId(), batch.value.id)
    } catch (error) {
      item.state = 'failed'
      item.error = error
      await refresh().catch(() => undefined)
      throw error
    } finally {
      running.value = false
    }
  }

  async function cancel() {
    if (!batch.value) return
    cancelling.value = true
    cancelRequested.value = true
    controllers.forEach((controller) => controller.abort())
    try {
      await uploadsApi.cancel(projectId(), batch.value.id)
      batch.value = await uploadsApi.get(projectId(), batch.value.id)
    } finally {
      running.value = false
      cancelling.value = false
    }
  }

  function reset() {
    if (running.value || cancelling.value) return
    batch.value = null
    queue.value = []
    cancelRequested.value = false
  }

  return {
    batch,
    queue,
    running,
    cancelling,
    totalLoaded,
    totalBytes,
    start,
    refresh,
    retry,
    cancel,
    reset,
  }
}

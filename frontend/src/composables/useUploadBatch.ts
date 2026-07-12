import { computed, ref } from 'vue'

import { getUploadConfig, uploadsApi, type UploadBatchDto } from '@/api/uploads'

export interface UploadQueueFile {
  file: File
  path: string
  loaded: number
  state: 'waiting' | 'uploading' | 'uploaded' | 'failed'
  error: unknown | null
}

/** Browser-side scheduler; file bytes are never parsed or copied into JSON. */
export function useUploadBatch(projectId: () => number) {
  const batch = ref<UploadBatchDto | null>(null)
  const queue = ref<UploadQueueFile[]>([])
  const running = ref(false)
  const totalLoaded = computed(() => queue.value.reduce((sum, item) => sum + item.loaded, 0))
  const totalBytes = computed(() => queue.value.reduce((sum, item) => sum + item.file.size, 0))

  async function start(files: File[]) {
    const config = await getUploadConfig()
    if (files.length === 0 || files.length > config.max_files_per_batch) {
      throw new Error('upload_file_count_exceeded')
    }
    const declared = files.map((file) => ({
      path: file.webkitRelativePath || file.name,
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
        while (nextIndex < queue.value.length) {
          const index = nextIndex++
          const item = queue.value[index]!
          const remote = batch.value!.files[index]!
          const attempt = remote.attempts[0]!
          item.state = 'uploading'
          try {
            await uploadsApi.receiveAttempt(
              projectId(),
              batch.value!.id,
              remote.id,
              attempt.id,
              item.file,
              (loaded) => (item.loaded = loaded),
            )
            item.loaded = item.file.size
            item.state = 'uploaded'
          } catch (error) {
            item.state = 'failed'
            item.error = error
          }
        }
      }
      await Promise.all(
        Array.from({ length: Math.min(config.client_concurrency, files.length) }, () => worker()),
      )
      if (queue.value.every((item) => item.state === 'uploaded')) {
        batch.value = await uploadsApi.complete(projectId(), batch.value.id)
      } else {
        batch.value = await uploadsApi.get(projectId(), batch.value.id)
      }
      return batch.value
    } finally {
      running.value = false
    }
  }

  async function cancel() {
    if (!batch.value) return
    await uploadsApi.cancel(projectId(), batch.value.id)
    batch.value = await uploadsApi.get(projectId(), batch.value.id)
    running.value = false
  }

  return { batch, queue, running, totalLoaded, totalBytes, start, cancel }
}

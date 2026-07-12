import { computed, onScopeDispose, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { jobsApi, type JobDto } from '@/api'

const ACTIVE_STATES = new Set(['queued', 'running', 'paused'])

/** Poll a durable job and stop automatically after it reaches a terminal state. */
export function useJobProgress(jobId: MaybeRefOrGetter<number | null | undefined>, intervalMs = 1500) {
  const job = ref<JobDto | null>(null)
  const loading = ref(false)
  const error = ref<unknown>(null)
  let timer: ReturnType<typeof setTimeout> | undefined
  let generation = 0

  const progress = computed(() => {
    const currentJob = job.value
    const total = currentJob?.progress_total
    if (!total || total <= 0) return null
    return Math.min(1, currentJob.progress_current / total)
  })
  const active = computed(() => job.value !== null && ACTIVE_STATES.has(job.value.state))

  function stop() {
    generation += 1
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }

  async function refresh() {
    const id = toValue(jobId)
    if (!id) return
    const requestGeneration = generation
    loading.value = true
    error.value = null
    try {
      const next = await jobsApi.get(id)
      if (requestGeneration !== generation) return
      job.value = next
      if (ACTIVE_STATES.has(next.state)) {
        timer = setTimeout(() => void refresh(), intervalMs)
      }
    } catch (reason) {
      if (requestGeneration === generation) error.value = reason
    } finally {
      if (requestGeneration === generation) loading.value = false
    }
  }

  watch(
    () => toValue(jobId),
    (id) => {
      stop()
      job.value = null
      error.value = null
      if (id) void refresh()
    },
    { immediate: true },
  )
  onScopeDispose(stop)

  return { job, loading, error, progress, active, refresh, stop }
}

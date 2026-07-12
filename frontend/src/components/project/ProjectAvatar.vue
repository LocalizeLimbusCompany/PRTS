<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'

import { projectsApi } from '@/api'

const props = withDefaults(
  defineProps<{
    projectId: number
    name: string
    avatarUrl: string | null
    avatarUpdatedAt?: string | null
    size?: string
  }>(),
  { avatarUpdatedAt: null, size: '54px' },
)

const objectUrl = ref<string | null>(null)
let loadSequence = 0

function revoke() {
  if (objectUrl.value) URL.revokeObjectURL(objectUrl.value)
  objectUrl.value = null
}

/** Always use the authenticated client; private-project media cannot be loaded by a plain img request. */
async function load() {
  const sequence = ++loadSequence
  revoke()
  if (!props.avatarUrl) return
  try {
    const blob = await projectsApi.avatar(props.projectId)
    if (sequence !== loadSequence) return
    objectUrl.value = URL.createObjectURL(blob)
  } catch {
    // The initials remain a stable fallback when media was concurrently removed.
  }
}

watch(
  () => [props.projectId, props.avatarUrl, props.avatarUpdatedAt],
  load,
  { immediate: true },
)
onBeforeUnmount(() => {
  loadSequence += 1
  revoke()
})
</script>

<template>
  <q-avatar square :size="size" class="project-avatar">
    <img v-if="objectUrl" :src="objectUrl" :alt="name" />
    <span v-else>{{ name.slice(0, 2).toUpperCase() }}</span>
  </q-avatar>
</template>

<style scoped>
.project-avatar {
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
  color: var(--prts-accent);
  font-family: var(--font-mono);
  font-size: 16px;
}

.project-avatar img {
  object-fit: cover;
}
</style>

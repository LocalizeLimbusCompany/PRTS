<script setup lang="ts">
import { computed } from 'vue'

import { STATE_ORDER } from '@/lib/states'

const props = withDefaults(
  defineProps<{
    stateCounts: Record<string, number>
    total: number
    compact?: boolean
  }>(),
  { compact: false },
)

const progress = computed<number | null>(() => {
  if (props.total === 0) return null
  return (props.total - (props.stateCounts.untranslated ?? 0)) / props.total
})
</script>

<template>
  <section class="project-progress" :class="{ 'project-progress--compact': props.compact }">
    <div class="project-progress__summary">
      <span class="prts-label">{{ $t('project.progress') }}</span>
      <span class="prts-mono">
        {{ progress === null ? '—' : `${Math.round(progress * 100)}%` }}
        <span v-if="!props.compact" class="prts-dim">· {{ props.total }} {{ $t('project.entries') }}</span>
      </span>
    </div>
    <q-linear-progress
      :value="progress ?? 0"
      :color="progress === null ? 'grey-7' : 'primary'"
      track-color="dark"
      size="6px"
    />
    <div v-if="!props.compact" class="project-progress__states">
      <div v-for="state in STATE_ORDER" :key="state" class="project-progress__state">
        <span class="state-dot" :class="`state-${state}`" />
        <span class="prts-dim">{{ $t(`project.states.${state}`) }}</span>
        <span class="prts-mono">{{ props.stateCounts[state] ?? 0 }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.project-progress {
  display: grid;
  gap: 10px;
}

.project-progress__summary,
.project-progress__states,
.project-progress__state {
  display: flex;
  align-items: center;
}

.project-progress__summary {
  justify-content: space-between;
  gap: 16px;
}

.project-progress__states {
  flex-wrap: wrap;
  gap: 8px 20px;
}

.project-progress__state {
  gap: 7px;
  font-size: 12px;
}

.project-progress--compact {
  gap: 6px;
  min-width: 112px;
}
</style>

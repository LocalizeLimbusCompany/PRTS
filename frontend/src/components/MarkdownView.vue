<script setup lang="ts">
import { computed } from 'vue'

import { renderSafeMarkdown } from '@/lib/markdown'

const props = withDefaults(
  defineProps<{
    source?: string
  }>(),
  { source: '' },
)

const rendered = computed(() => renderSafeMarkdown(props.source))
</script>

<template>
  <!-- eslint-disable-next-line vue/no-v-html -- rendered is always sanitized by renderSafeMarkdown. -->
  <div class="prts-markdown" v-html="rendered" />
</template>

<style scoped>
.prts-markdown :deep(:first-child) {
  margin-top: 0;
}

.prts-markdown :deep(:last-child) {
  margin-bottom: 0;
}

.prts-markdown :deep(h1),
.prts-markdown :deep(h2),
.prts-markdown :deep(h3) {
  color: var(--prts-text-strong);
  font-family: var(--font-sans);
  line-height: 1.35;
}

.prts-markdown :deep(pre),
.prts-markdown :deep(code) {
  font-family: var(--font-mono);
}

.prts-markdown :deep(pre) {
  overflow: auto;
  padding: 12px;
  background: var(--prts-panel-2);
  border: 1px solid var(--prts-border);
  border-radius: 2px;
}
</style>

<script lang="ts">
import type { StructuredSearchRequest } from '@/api'

/** 把一次快捷键提交转换成 exact structured scope；IME composing 时返回 null。 */
export function quickSearchRequest(
  rawQuery: string,
  currentFileOnly: boolean,
  composing: boolean,
  currentFileId: number | null,
): StructuredSearchRequest | null {
  const query = rawQuery.trim()
  if (composing || !query) return null
  if (currentFileOnly && (!Number.isInteger(currentFileId) || (currentFileId ?? 0) <= 0)) {
    return null
  }
  return {
    query,
    conditions: [],
    scope: currentFileOnly
      ? { type: 'current_file', file_id: currentFileId as number }
      : { type: 'all' },
    states: [],
    include_hidden: false,
    vector: false,
    limit: 50,
  }
}
</script>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { StructuredSearchRequest as SearchRequest } from '@/api'

const props = defineProps<{ currentFileId: number | null }>()
const emit = defineEmits<{
  (event: 'search', request: SearchRequest): void
  (event: 'clear'): void
}>()
const { t } = useI18n()
const query = ref('')

function submit(event: KeyboardEvent) {
  if (event.key !== 'Enter') return
  const request = quickSearchRequest(
    query.value,
    event.shiftKey,
    event.isComposing,
    props.currentFileId,
  )
  if (request) emit('search', request)
}

function clear() {
  query.value = ''
  emit('clear')
}
</script>

<template>
  <q-input
    v-model="query"
    dense
    outlined
    clearable
    :placeholder="t('editor.searchPlaceholder')"
    @keydown="submit"
    @clear="clear"
  >
    <template #prepend><q-icon name="mdi-magnify" /></template>
    <q-tooltip>{{ t('editor.searchShortcutHint') }}</q-tooltip>
  </q-input>
</template>

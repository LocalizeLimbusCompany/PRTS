<script lang="ts">
import { displayPosName } from '@/lib/terminology'

export function insertTermSuggestion(
  value: string,
  selectionStart: number,
  selectionEnd: number,
  insertion: string,
): { value: string; cursor: number } {
  const start = Math.max(0, Math.min(selectionStart, value.length))
  const end = Math.max(start, Math.min(selectionEnd, value.length))
  return {
    value: value.slice(0, start) + insertion + value.slice(end),
    cursor: start + insertion.length,
  }
}

export function termPosName(
  term: { pos_name_zh_cn: string | null; pos_name_en: string | null },
  locale: string,
): string {
  return displayPosName({ name_zh_cn: term.pos_name_zh_cn, name_en: term.pos_name_en }, locale)
}
</script>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import type { TermDto } from '@/api'

defineProps<{ terms: TermDto[] }>()
const emit = defineEmits<{ (event: 'apply', translation: string): void }>()
const { locale, t } = useI18n()
</script>

<template>
  <div v-if="terms.length" class="term-suggestions">
    <div class="prts-label q-mb-xs">{{ t('editor.termSuggestions') }}</div>
    <div class="row q-col-gutter-sm">
      <button
        v-for="term in terms"
        :key="term.id"
        type="button"
        class="term-suggestion"
        @click="emit('apply', term.translation)"
      >
        <span>{{ term.source_text }}</span>
        <span class="term-arrow">→</span>
        <span>{{ term.translation }}</span>
        <span v-if="termPosName(term, locale)" class="prts-dim">{{
          termPosName(term, locale)
        }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.term-suggestions {
  margin-top: 14px;
}
.term-suggestion {
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
  color: var(--prts-text);
  border-radius: 4px;
  padding: 6px 8px;
  cursor: pointer;
}
.term-suggestion:hover {
  border-color: var(--prts-accent);
}
.term-arrow {
  margin: 0 6px;
  color: var(--prts-accent);
}
</style>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import type { TermDto } from '@/api'
import { displayPosName } from '@/lib/terminology'

defineProps<{ terms: TermDto[] }>()
const emit = defineEmits<{ apply: [translation: string] }>()
const { locale } = useI18n()
</script>

<template>
  <div class="context-list">
    <button
      v-for="term in terms"
      :key="term.id"
      type="button"
      class="term-card"
      @click="emit('apply', term.translation)"
    >
      <span class="term-card__source">{{ term.source_text }}</span>
      <q-icon name="mdi-arrow-right" size="15px" />
      <span class="term-card__target">{{ term.translation }}</span>
      <span
        v-if="
          displayPosName({ name_zh_cn: term.pos_name_zh_cn, name_en: term.pos_name_en }, locale)
        "
        class="term-card__meta"
      >
        {{ displayPosName({ name_zh_cn: term.pos_name_zh_cn, name_en: term.pos_name_en }, locale) }}
      </span>
      <span v-if="term.notes" class="term-card__notes">{{ term.notes }}</span>
    </button>
    <div v-if="terms.length === 0" class="prts-empty">{{ $t('editor.noTerms') }}</div>
  </div>
</template>

<style scoped>
.context-list {
  display: grid;
  gap: 8px;
  padding: 10px;
}
.term-card {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
  gap: 7px;
  align-items: center;
  width: 100%;
  padding: 10px;
  border: 1px solid var(--prts-border-soft);
  border-radius: 3px;
  color: var(--prts-text);
  background: var(--prts-panel-2);
  text-align: left;
  cursor: pointer;
}
.term-card:hover {
  border-color: var(--prts-accent);
}
.term-card__source,
.term-card__target {
  overflow-wrap: anywhere;
}
.term-card__target {
  color: var(--prts-text-strong);
}
.term-card__meta,
.term-card__notes {
  grid-column: 1 / -1;
  color: var(--prts-text-dim);
  font-size: 11px;
}
</style>

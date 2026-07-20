<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { TermDto } from '@/api'
import { displayPosName, segmentSourceTerms } from '@/lib/terminology'

const props = defineProps<{ source: string; terms: TermDto[] }>()
const emit = defineEmits<{ apply: [translation: string] }>()
const { locale, t } = useI18n()
const segments = computed(() => segmentSourceTerms(props.source, props.terms))
</script>

<template>
  <span class="source-term-text">
    <template v-for="(segment, index) in segments" :key="index">
      <button
        v-if="segment.term"
        type="button"
        class="source-term-text__term"
        @click="emit('apply', segment.term.translation)"
      >
        {{ segment.text }}
        <q-tooltip class="source-term-tooltip">
          <strong>{{ segment.term.source_text }} → {{ segment.term.translation }}</strong>
          <span
            v-if="
              displayPosName(
                { name_zh_cn: segment.term.pos_name_zh_cn, name_en: segment.term.pos_name_en },
                locale,
              )
            "
          >
            {{
              displayPosName(
                { name_zh_cn: segment.term.pos_name_zh_cn, name_en: segment.term.pos_name_en },
                locale,
              )
            }}
          </span>
          <span v-if="segment.term.notes">{{ segment.term.notes }}</span>
          <small>{{ t('editor.clickTermToInsert') }}</small>
        </q-tooltip>
      </button>
      <template v-else>{{ segment.text }}</template>
    </template>
  </span>
</template>

<style scoped>
.source-term-text__term {
  appearance: none;
  padding: 0;
  border: 0;
  border-bottom: 1px solid var(--prts-accent);
  color: inherit;
  background: transparent;
  font: inherit;
  cursor: pointer;
}
.source-term-text__term:hover {
  color: var(--prts-accent);
}
:global(.source-term-tooltip) {
  display: grid;
  gap: 4px;
  max-width: 320px;
}
</style>

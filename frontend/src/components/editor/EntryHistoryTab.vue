<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { EntryDiffMode, EntryVersionDto } from '@/api'
import { diffText } from '@/lib/editorDiff'
import { stateLabel } from '@/lib/states'

const props = defineProps<{
  history: EntryVersionDto[]
  mode: EntryDiffMode
  primarySource: string | null
}>()
const { t } = useI18n()
const comparisons = computed(() =>
  props.history.map((current, index) => {
    const previous = props.history[index + 1]
    const beforeTranslation = previous?.translation ?? ''
    const beforeSource = props.primarySource ? (previous?.original[props.primarySource] ?? '') : ''
    const afterSource = props.primarySource ? (current.original[props.primarySource] ?? '') : ''
    const translationChanged = beforeTranslation !== current.translation
    const sourceChanged = beforeSource !== afterSource
    const stateChanged = Boolean(previous && previous.state !== current.state)
    const questionedChanged = Boolean(previous && previous.questioned !== current.questioned)
    return {
      current,
      previous,
      translation:
        props.mode === 'side_by_side'
          ? []
          : diffText(
              beforeTranslation,
              current.translation,
              props.mode === 'character_inline' ? 'character' : 'word',
            ),
      source:
        props.mode === 'side_by_side'
          ? []
          : diffText(
              beforeSource,
              afterSource,
              props.mode === 'character_inline' ? 'character' : 'word',
            ),
      beforeTranslation,
      beforeSource,
      afterSource,
      translationChanged,
      sourceChanged,
      stateChanged,
      questionedChanged,
    }
  }),
)
</script>

<template>
  <div class="history-list">
    <article
      v-for="item in comparisons"
      :key="`${item.current.version}-${item.current.kind}`"
      class="history-card"
    >
      <header class="history-card__head">
        <q-avatar size="28px" color="primary" text-color="dark">
          <img v-if="item.current.editor_avatar_url" :src="item.current.editor_avatar_url" alt="" />
          <span v-else>{{ item.current.editor_name?.charAt(0).toUpperCase() ?? '?' }}</span>
        </q-avatar>
        <div>
          <strong>{{ item.current.editor_name ?? $t('editor.systemActor') }}</strong>
          <div class="prts-dim">{{ new Date(item.current.created_at).toLocaleString() }}</div>
        </div>
        <q-space />
        <q-badge outline :label="stateLabel(item.current.state, t)" />
      </header>
      <div v-if="item.stateChanged" class="history-card__change">
        {{ $t('editor.stateChanged') }}：{{ stateLabel(item.previous!.state, t) }} →
        {{ stateLabel(item.current.state, t) }}
      </div>
      <div v-if="item.questionedChanged" class="history-card__change">
        {{ $t('editor.questionedChanged') }}：
        {{ $t(item.current.questioned ? 'editor.questionedAdded' : 'editor.questionedRemoved') }}
      </div>
      <template v-if="item.translationChanged">
        <div class="prts-label">{{ $t('editor.translationDiff') }}</div>
        <div v-if="mode === 'side_by_side'" class="history-card__side">
          <pre>{{ item.beforeTranslation }}</pre>
          <pre>{{ item.current.translation }}</pre>
        </div>
        <div v-else class="history-card__diff">
          <span
            v-for="(part, index) in item.translation"
            :key="index"
            :class="`diff-${part.kind}`"
            >{{ part.text }}</span
          >
        </div>
      </template>
      <template v-if="item.sourceChanged">
        <div class="prts-label">{{ $t('editor.sourceDiff') }}</div>
        <div v-if="mode === 'side_by_side'" class="history-card__side">
          <pre>{{ item.beforeSource }}</pre>
          <pre>{{ item.afterSource }}</pre>
        </div>
        <div v-else class="history-card__diff">
          <span v-for="(part, index) in item.source" :key="index" :class="`diff-${part.kind}`">{{
            part.text
          }}</span>
        </div>
      </template>
    </article>
    <div v-if="history.length === 0" class="prts-empty">{{ $t('editor.noHistory') }}</div>
  </div>
</template>

<style scoped>
.history-list {
  display: grid;
  gap: 10px;
  padding: 10px;
}
.history-card {
  display: grid;
  gap: 8px;
  padding: 10px;
  border: 1px solid var(--prts-border-soft);
  background: var(--prts-panel-2);
}
.history-card__head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.history-card__diff {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.history-card__change {
  padding: 7px 9px;
  border-left: 2px solid var(--prts-accent);
  background: var(--prts-accent-dim);
}
.history-card__side {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
.history-card__side pre {
  min-width: 0;
  margin: 0;
  padding: 7px;
  white-space: pre-wrap;
  background: var(--prts-bg-elev);
}
.diff-insert {
  color: var(--q-positive);
  background: rgba(63, 185, 80, 0.13);
  text-decoration: underline;
}
.diff-delete {
  color: var(--q-negative);
  background: rgba(248, 81, 73, 0.12);
  text-decoration: line-through;
}
</style>

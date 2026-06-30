<script setup lang="ts">
/**
 * SuggestionsPanel.vue — 编辑器下方的 TM 翻译建议面板。
 *
 * 接收来自 TM 接口的建议列表，每条建议显示原文、译文、来源项目及相似度；
 * 点击「应用」按钮时向父组件 emit apply 事件，携带该条译文。
 */
import { useI18n } from 'vue-i18n'
import type { SuggestionDto } from '@/api'

const props = defineProps<{
  suggestions: SuggestionDto[]
}>()

const emit = defineEmits<{
  /** 用户点击某条建议时，携带对应译文。 */
  (e: 'apply', translation: string): void
}>()

const { t } = useI18n()

/** 将相似度浮点值转为百分比整数展示。 */
function simPct(similarity: number): number {
  return Math.round(similarity * 100)
}
</script>

<template>
  <!-- 无建议时不渲染任何内容 -->
  <template v-if="props.suggestions.length">
    <div class="sg-header">
      <span class="prts-label">{{ t('suggestions.title') }}</span>
    </div>
    <div class="sg-list">
      <q-card
        v-for="sg in props.suggestions"
        :key="sg.entry_id"
        flat
        bordered
        class="sg-card"
        @click="emit('apply', sg.translation)"
      >
        <!-- 相似度徽标 + 来源 -->
        <div class="sg-card__meta row items-center q-gutter-x-sm">
          <q-badge
            color="primary"
            text-color="dark"
            class="prts-mono"
            :label="simPct(sg.similarity) + '%'"
          />
          <span class="sg-card__project prts-dim">{{ sg.project_name }}</span>
          <q-space />
          <q-btn
            flat
            dense
            no-caps
            size="sm"
            color="primary"
            :label="t('suggestions.apply')"
            @click.stop="emit('apply', sg.translation)"
          />
        </div>
        <!-- 原文（较小） -->
        <div class="sg-card__source prts-dim">{{ sg.source_text }}</div>
        <!-- 译文（突出显示） -->
        <div class="sg-card__translation">{{ sg.translation }}</div>
      </q-card>
    </div>
  </template>
</template>

<style scoped>
.sg-header {
  margin-top: 18px;
  margin-bottom: 6px;
}

.sg-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.sg-card {
  padding: 10px 14px;
  cursor: pointer;
  transition: background 0.15s;
}

.sg-card:hover {
  background: var(--prts-panel-2);
}

.sg-card__meta {
  margin-bottom: 4px;
}

.sg-card__project {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 180px;
}

.sg-card__source {
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  margin-bottom: 4px;
}

.sg-card__translation {
  font-size: 14px;
  line-height: 1.6;
  color: var(--prts-text-strong);
  white-space: pre-wrap;
}
</style>

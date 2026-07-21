<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { AiUiLocale } from '@/api'
import { useAiExplanationSessionStore } from '@/stores/aiExplanationSession'

const props = defineProps<{ projectId: number; entryId: number }>()
const { locale, t } = useI18n()
const sessions = useAiExplanationSessionStore()

const activeUiLocale = computed<AiUiLocale>(() => (locale.value === 'en' ? 'en' : 'zh-CN'))
const session = computed(() =>
  sessions.getOrCreate(props.projectId, props.entryId, activeUiLocale.value),
)
const explanation = computed(() => session.value.result)

const tokenLabel = computed(() =>
  t(session.value.outputTokensExact ? 'editor.ai.tokensExact' : 'editor.ai.tokensEstimated', {
    count: session.value.outputTokens,
  }),
)
const resultTokenLabel = computed(() => {
  const result = explanation.value
  if (!result || result.output_tokens === null) return ''
  return t(result.output_tokens_exact ? 'editor.ai.tokensExact' : 'editor.ai.tokensEstimated', {
    count: result.output_tokens,
  })
})

function citationDomain(url: string): string {
  try {
    return new URL(url).hostname
  } catch {
    return url
  }
}

/** Cancelling the browser request also closes the backend channel and upstream provider stream. */
function cancelAnalysis() {
  sessions.cancel(props.projectId, props.entryId, activeUiLocale.value)
}

/** AI 只在用户明确点击后分析当前词条的主源文本。 */
function explain() {
  void sessions.analyze(props.projectId, props.entryId, activeUiLocale.value)
}
</script>

<template>
  <div class="ai-panel">
    <div v-if="session.loading" class="ai-panel__progress" role="status" aria-live="polite">
      <div class="ai-panel__progress-head">
        <q-icon name="mdi-brain" size="30px" color="primary" />
        <div>
          <div class="prts-h2">{{ t(`editor.ai.phases.${session.phase}`) }}</div>
          <div class="prts-dim q-mt-xs">
            {{ t('editor.ai.elapsed', { seconds: session.elapsedSeconds }) }} · {{ tokenLabel }}
          </div>
        </div>
      </div>
      <q-linear-progress indeterminate rounded size="6px" color="primary" />
      <q-btn
        outline
        no-caps
        color="negative"
        icon="mdi-stop-circle-outline"
        :label="t('editor.ai.cancel')"
        :aria-label="t('editor.ai.cancel')"
        @click="cancelAnalysis"
      />
    </div>
    <q-banner v-if="session.errorCode" dense class="bg-negative text-white">
      {{ session.errorMessage || t('editor.ai.loadFailed') }}
    </q-banner>
    <q-banner v-else-if="session.cancelled" dense class="ai-panel__notice">
      {{ t('editor.ai.cancelled') }}
    </q-banner>
    <div v-if="!session.loading && !explanation" class="ai-panel__empty">
      <q-icon name="mdi-auto-fix" size="34px" color="primary" />
      <div class="prts-h2">{{ t('editor.ai.heading') }}</div>
      <div class="prts-dim">{{ t('editor.ai.description') }}</div>
      <q-btn
        unelevated
        no-caps
        color="primary"
        text-color="dark"
        icon="mdi-auto-fix"
        :label="t('editor.ai.analyze')"
        @click="explain"
      />
    </div>
    <template v-if="explanation">
      <div class="ai-panel__head">
        <div>
          <div class="prts-label">{{ t('editor.ai.overallMeaning') }}</div>
          <div class="ai-panel__overall">{{ explanation.overall_meaning }}</div>
        </div>
        <q-btn
          flat
          round
          dense
          icon="mdi-refresh"
          :loading="session.loading"
          :aria-label="t('editor.ai.analyzeAgain')"
          @click="explain"
        />
      </div>

      <div v-if="explanation.grammar_notes" class="ai-panel__grammar">
        <div class="prts-label">{{ t('editor.ai.grammar') }}</div>
        <div>{{ explanation.grammar_notes }}</div>
      </div>

      <div class="prts-label">{{ t('editor.ai.tokens') }}</div>
      <div class="ai-token-list">
        <article v-for="token in explanation.tokens" :key="token.token" class="ai-token">
          <header class="ai-token__head">
            <strong>{{ token.token }}</strong>
            <q-badge v-if="token.part_of_speech" outline color="primary">
              {{ token.part_of_speech }}
            </q-badge>
          </header>
          <div class="ai-token__meaning">{{ token.meaning }}</div>
          <div v-if="token.contextual_explanation" class="prts-dim">
            {{ token.contextual_explanation }}
          </div>
          <div v-if="token.grammar_notes" class="ai-token__notes">
            {{ token.grammar_notes }}
          </div>
        </article>
        <div v-if="!explanation.tokens.length" class="prts-empty">
          {{ t('editor.ai.noTokens') }}
        </div>
      </div>

      <div class="ai-panel__meta prts-dim">
        {{
          t('editor.ai.provider', {
            source: t(`profile.ai.sources.${explanation.provider_source}`),
          })
        }}
        <span v-if="explanation.cached"> · {{ t('editor.ai.cached') }}</span>
        <span v-if="resultTokenLabel"> · {{ resultTokenLabel }}</span>
      </div>

      <q-banner
        dense
        class="ai-panel__search-status"
        :class="{ 'ai-panel__search-status--warning': explanation.search_status === 'failed' }"
      >
        <q-icon :name="explanation.search_used ? 'mdi-web-check' : 'mdi-web-off'" size="18px" />
        <span>{{ t(`editor.ai.searchStatuses.${explanation.search_status}`) }}</span>
        <span v-if="explanation.search_provider" class="prts-dim">
          · {{ explanation.search_provider }}
        </span>
      </q-banner>

      <q-expansion-item
        v-if="explanation.citations.length"
        dense
        dense-toggle
        icon="mdi-link-variant"
        :label="t('editor.ai.citations', { count: explanation.citations.length })"
      >
        <ol class="ai-citations">
          <li v-for="citation in explanation.citations" :key="citation.url">
            <a :href="citation.url" target="_blank" rel="noopener noreferrer">
              {{ citation.title }}
            </a>
            <span class="ai-citations__domain">{{ citationDomain(citation.url) }}</span>
            <p v-if="citation.snippet">{{ citation.snippet }}</p>
          </li>
        </ol>
      </q-expansion-item>
    </template>
  </div>
</template>

<style scoped>
.ai-panel {
  min-width: 0;
  display: grid;
  gap: 14px;
  padding: 12px;
}

.ai-panel__empty {
  display: grid;
  justify-items: start;
  gap: 10px;
  padding: 20px 8px;
}

.ai-panel__progress {
  display: grid;
  justify-items: stretch;
  gap: 16px;
  padding: 20px 8px;
}

.ai-panel__progress-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.ai-panel__progress > .q-btn {
  justify-self: start;
}

.ai-panel__head,
.ai-token__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.ai-panel__overall {
  margin-top: 5px;
  color: var(--prts-text-strong);
  line-height: 1.65;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.ai-panel__grammar,
.ai-token {
  display: grid;
  gap: 6px;
  padding: 10px;
  border: 1px solid var(--prts-border-soft);
  background: var(--prts-panel-2);
  line-height: 1.55;
}

.ai-panel__notice,
.ai-panel__search-status {
  border: 1px solid var(--prts-border-soft);
  background: var(--prts-panel-2);
}

.ai-panel__search-status--warning {
  border-color: color-mix(in srgb, var(--q-warning) 55%, var(--prts-border));
}

.ai-citations {
  display: grid;
  gap: 12px;
  margin: 8px 0 0;
  padding-left: 28px;
}

.ai-citations li,
.ai-citations p,
.ai-citations a {
  min-width: 0;
  overflow-wrap: anywhere;
}

.ai-citations a {
  color: var(--prts-accent);
}

.ai-citations__domain {
  margin-left: 6px;
  color: var(--prts-text-dim);
  font-size: 11px;
}

.ai-citations p {
  margin: 4px 0 0;
  color: var(--prts-text-dim);
}

.ai-token-list {
  display: grid;
  gap: 8px;
}

.ai-token__meaning {
  color: var(--prts-text-strong);
}

.ai-token__notes {
  padding-top: 5px;
  border-top: 1px solid var(--prts-border-soft);
  color: var(--prts-text-dim);
  font-size: 12px;
}

.ai-panel__meta {
  font-size: 11px;
}

@media (max-width: 390px) {
  .ai-panel__progress > .q-btn {
    width: 40px;
    min-height: 40px;
    padding: 0;
  }

  .ai-panel__progress > .q-btn :deep(.q-btn__content > .block) {
    display: none;
  }

  .ai-citations__domain {
    display: block;
    margin-left: 0;
  }
}
</style>

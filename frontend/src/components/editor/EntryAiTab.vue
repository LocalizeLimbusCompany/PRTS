<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { aiApi, apiErrorMessage, type AiExplanationDto, type AiStreamPhase } from '@/api'

const props = defineProps<{ projectId: number; entryId: number }>()
const $q = useQuasar()
const { t } = useI18n()
const loading = ref(false)
const explanation = ref<AiExplanationDto | null>(null)
const phase = ref<AiStreamPhase>('connecting')
const outputTokens = ref(0)
const outputTokensExact = ref(false)
const elapsedSeconds = ref(0)
const controller = ref<AbortController | null>(null)
let elapsedTimer: ReturnType<typeof setInterval> | undefined

const tokenLabel = computed(() =>
  t(outputTokensExact.value ? 'editor.ai.tokensExact' : 'editor.ai.tokensEstimated', {
    count: outputTokens.value,
  }),
)

function stopElapsedTimer() {
  if (elapsedTimer) clearInterval(elapsedTimer)
  elapsedTimer = undefined
}

/** Cancelling the browser request also closes the backend channel and upstream provider stream. */
function cancelAnalysis() {
  controller.value?.abort()
}

watch(
  () => [props.projectId, props.entryId],
  () => {
    cancelAnalysis()
    explanation.value = null
    outputTokens.value = 0
    outputTokensExact.value = false
  },
)

onBeforeUnmount(() => {
  cancelAnalysis()
  stopElapsedTimer()
})

/** AI 只在用户明确点击后分析当前词条的主源文本。 */
async function explain() {
  cancelAnalysis()
  const requestController = new AbortController()
  controller.value = requestController
  loading.value = true
  explanation.value = null
  phase.value = 'connecting'
  outputTokens.value = 0
  outputTokensExact.value = false
  elapsedSeconds.value = 0
  stopElapsedTimer()
  const startedAt = Date.now()
  elapsedTimer = setInterval(() => {
    elapsedSeconds.value = Math.floor((Date.now() - startedAt) / 1_000)
  }, 1_000)
  try {
    const result = await aiApi.streamExplainEntry(
      props.projectId,
      props.entryId,
      undefined,
      {
        onStatus(status) {
          if (controller.value === requestController) phase.value = status.phase
        },
        onProgress(progress) {
          if (controller.value !== requestController) return
          phase.value = progress.phase
          outputTokens.value = progress.estimated_output_tokens
          outputTokensExact.value = false
        },
      },
      requestController.signal,
    )
    if (controller.value !== requestController) return
    explanation.value = result
    outputTokens.value = result.output_tokens ?? outputTokens.value
    outputTokensExact.value = result.output_tokens_exact
  } catch (error) {
    if (requestController.signal.aborted) return
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('editor.ai.loadFailed')) })
  } finally {
    if (controller.value === requestController) {
      controller.value = null
      loading.value = false
      stopElapsedTimer()
    }
  }
}
</script>

<template>
  <div class="ai-panel">
    <div v-if="loading" class="ai-panel__progress" role="status" aria-live="polite">
      <div class="ai-panel__progress-head">
        <q-icon name="mdi-brain" size="30px" color="primary" />
        <div>
          <div class="prts-h2">{{ t(`editor.ai.phases.${phase}`) }}</div>
          <div class="prts-dim q-mt-xs">
            {{ t('editor.ai.elapsed', { seconds: elapsedSeconds }) }} · {{ tokenLabel }}
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
        @click="cancelAnalysis"
      />
    </div>
    <div v-else-if="!explanation" class="ai-panel__empty">
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
    <template v-else>
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
          :loading="loading"
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
        <span v-if="explanation.output_tokens !== null"> · {{ tokenLabel }}</span>
      </div>
    </template>
  </div>
</template>

<style scoped>
.ai-panel {
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
</style>

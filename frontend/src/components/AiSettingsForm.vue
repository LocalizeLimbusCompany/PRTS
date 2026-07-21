<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import type {
  AiProviderPreset,
  AiReasoningEffort,
  AiSettingsDto,
  AiSettingsWriteRequest,
  WebSearchMode,
} from '@/api'
import {
  AiSettingsValidationError,
  buildAiSettingsRequest,
  createAiSettingsForm,
} from '@/lib/aiSettings'

const props = withDefaults(
  defineProps<{
    settings: AiSettingsDto | null
    loading?: boolean
    scope: 'personal' | 'project'
  }>(),
  { loading: false },
)
const emit = defineEmits<{
  save: [request: AiSettingsWriteRequest]
  delete: []
}>()
const { t } = useI18n()
const form = ref(createAiSettingsForm(props.settings))
const validationCode = ref<string | null>(null)

watch(
  () => props.settings,
  (settings) => {
    form.value = createAiSettingsForm(settings)
    validationCode.value = null
  },
)

watch(
  () => form.value.provider_preset,
  (preset) => {
    if (preset === 'custom') form.value.thinking_mode = 'auto'
    if (preset === 'deepseek' && !['high', 'max'].includes(form.value.reasoning_effort)) {
      form.value.reasoning_effort = 'high'
    }
    if ((preset === 'openai' || preset === 'gemini') && form.value.reasoning_effort === 'max') {
      form.value.reasoning_effort = 'medium'
    }
    validationCode.value = null
  },
)

const providerOptions = computed(() =>
  (['openai', 'qwen', 'deepseek', 'gemini', 'custom'] as AiProviderPreset[]).map((value) => ({
    value,
    label: t(`profile.ai.presets.${value}`),
  })),
)
const thinkingOptions = computed(() =>
  (['auto', 'enabled', 'disabled'] as const).map((value) => ({
    value,
    label: t(`profile.ai.thinkingModes.${value}`),
  })),
)
const effortValues = computed<AiReasoningEffort[]>(() =>
  form.value.provider_preset === 'deepseek' ? ['high', 'max'] : ['low', 'medium', 'high'],
)
const effortOptions = computed(() =>
  effortValues.value.map((value) => ({
    value,
    label: t(`profile.ai.reasoningEfforts.${value}`),
  })),
)
const searchModeOptions = computed(() =>
  (['disabled', 'adapter', 'native', 'auto'] as WebSearchMode[]).map((value) => ({
    value,
    label: t(`profile.ai.webSearch.modes.${value}`),
  })),
)
const searchProviderOptions = computed(() =>
  ['tavily', 'brave', 'serper', 'searxng'].map((value) => ({
    value,
    label: value === 'searxng' ? 'SearXNG' : value[0]?.toUpperCase() + value.slice(1),
  })),
)
const supportsEffort = computed(
  () =>
    form.value.provider_preset === 'openai' ||
    form.value.provider_preset === 'gemini' ||
    form.value.provider_preset === 'deepseek',
)
const obviousFieldsReady = computed(
  () =>
    Boolean(form.value.base_url.trim()) &&
    Boolean(form.value.model.trim()) &&
    Boolean(props.settings?.configured || form.value.api_key.trim()),
)

function submit() {
  try {
    const request = buildAiSettingsRequest(
      form.value,
      Boolean(props.settings?.configured),
      Boolean(props.settings?.web_search_configured),
    )
    validationCode.value = null
    emit('save', request)
  } catch (error) {
    validationCode.value = error instanceof AiSettingsValidationError ? error.code : 'json'
  }
}
</script>

<template>
  <div class="ai-settings-form">
    <div class="prts-dim">
      {{ t(scope === 'personal' ? 'profile.ai.description' : 'project.ai.description') }}
    </div>
    <q-banner v-if="settings?.configured" dense class="ai-settings-form__key">
      {{
        t(scope === 'personal' ? 'profile.ai.keyConfigured' : 'project.ai.keyConfigured', {
          hint: settings.api_key_hint,
        })
      }}
    </q-banner>
    <q-banner v-if="validationCode" dense class="bg-negative text-white">
      {{ t(`profile.ai.validation.${validationCode}`) }}
    </q-banner>

    <div class="ai-settings-form__grid">
      <q-select
        v-model="form.provider_preset"
        outlined
        dense
        emit-value
        map-options
        :options="providerOptions"
        :label="t('profile.ai.providerPreset')"
        :disable="loading"
      />
      <q-input
        v-model="form.model"
        outlined
        dense
        :label="t('profile.ai.model')"
        :disable="loading"
      />
      <q-input
        v-model="form.base_url"
        class="ai-settings-form__wide"
        outlined
        dense
        type="url"
        autocomplete="url"
        :label="t('profile.ai.baseUrl')"
        :hint="t('profile.ai.baseUrlHint')"
        :disable="loading"
      />
      <q-input
        v-model="form.api_key"
        class="ai-settings-form__wide"
        outlined
        dense
        type="password"
        autocomplete="new-password"
        :label="t('profile.ai.apiKey')"
        :hint="
          settings?.configured
            ? t('profile.ai.apiKeyRetainHint')
            : t('profile.ai.apiKeyRequiredHint')
        "
        :disable="loading"
      />
    </div>

    <q-separator />
    <div class="ai-settings-form__section">
      <div>
        <div class="prts-label">{{ t('profile.ai.thinkingMode') }}</div>
        <div class="prts-dim q-mt-xs">
          {{
            t(
              form.provider_preset === 'custom'
                ? 'profile.ai.customThinkingHint'
                : 'profile.ai.thinkingModeHint',
            )
          }}
        </div>
      </div>
      <q-btn-toggle
        v-model="form.thinking_mode"
        no-caps
        unelevated
        spread
        toggle-color="primary"
        toggle-text-color="dark"
        color="grey-9"
        :options="thinkingOptions"
        :disable="loading || form.provider_preset === 'custom'"
      />
      <div
        v-if="form.thinking_mode === 'enabled' && supportsEffort"
        class="ai-settings-form__control"
      >
        <div class="prts-label">{{ t('profile.ai.reasoningEffort') }}</div>
        <q-btn-toggle
          v-model="form.reasoning_effort"
          no-caps
          unelevated
          spread
          toggle-color="secondary"
          :options="effortOptions"
          :disable="loading"
        />
      </div>
      <q-input
        v-if="form.provider_preset === 'qwen' && form.thinking_mode === 'enabled'"
        v-model="form.thinking_budget"
        outlined
        dense
        type="number"
        min="1"
        max="1000000"
        step="1"
        :label="t('profile.ai.thinkingBudget')"
        :hint="t('profile.ai.thinkingBudgetHint')"
        :disable="loading"
      />
    </div>

    <q-input
      v-model="form.request_timeout_seconds"
      outlined
      dense
      type="number"
      min="30"
      max="600"
      step="10"
      :label="t('profile.ai.requestTimeout')"
      :hint="t('profile.ai.requestTimeoutHint')"
      :suffix="t('profile.ai.seconds')"
      :disable="loading"
    />

    <q-expansion-item
      dense
      dense-toggle
      icon="mdi-code-json"
      :label="t('profile.ai.customOptions')"
      :caption="t('profile.ai.customOptionsHint')"
    >
      <q-input
        v-model="form.custom_request_options"
        class="q-mt-sm"
        outlined
        type="textarea"
        rows="6"
        spellcheck="false"
        input-class="prts-mono ai-settings-form__json"
        :disable="loading"
      />
    </q-expansion-item>

    <q-separator />
    <div class="ai-settings-form__section">
      <div>
        <div class="prts-label">{{ t('profile.ai.webSearch.heading') }}</div>
        <div class="prts-dim q-mt-xs">{{ t('profile.ai.webSearch.description') }}</div>
      </div>
      <q-select
        v-model="form.web_search_mode"
        outlined
        dense
        emit-value
        map-options
        :options="searchModeOptions"
        :label="t('profile.ai.webSearch.mode')"
        :disable="loading"
      />
      <template v-if="form.web_search_mode === 'adapter' || form.web_search_mode === 'auto'">
        <q-select
          v-model="form.web_search_provider"
          outlined
          dense
          emit-value
          map-options
          :options="searchProviderOptions"
          :label="t('profile.ai.webSearch.provider')"
          :disable="loading"
        />
        <q-input
          v-model="form.web_search_endpoint"
          outlined
          dense
          type="url"
          autocomplete="url"
          :label="t('profile.ai.webSearch.endpoint')"
          :hint="t('profile.ai.webSearch.endpointHint')"
          :disable="loading"
        />
        <q-banner v-if="settings?.web_search_configured" dense class="ai-settings-form__key">
          {{
            t('profile.ai.webSearch.keyConfigured', {
              hint: settings.web_search_api_key_hint,
            })
          }}
        </q-banner>
        <q-input
          v-if="form.web_search_provider !== 'searxng'"
          v-model="form.web_search_api_key"
          outlined
          dense
          type="password"
          autocomplete="new-password"
          :label="t('profile.ai.webSearch.apiKey')"
          :hint="
            settings?.web_search_configured
              ? t('profile.ai.apiKeyRetainHint')
              : t('profile.ai.apiKeyRequiredHint')
          "
          :disable="loading"
        />
      </template>
      <div v-if="form.web_search_mode !== 'disabled'" class="ai-settings-form__grid">
        <q-input
          v-model="form.web_search_timeout_seconds"
          outlined
          dense
          type="number"
          min="3"
          max="60"
          :label="t('profile.ai.webSearch.timeout')"
          :suffix="t('profile.ai.seconds')"
          :disable="loading"
        />
        <q-input
          v-model="form.web_search_max_results"
          outlined
          dense
          type="number"
          min="1"
          max="10"
          :label="t('profile.ai.webSearch.maxResults')"
          :disable="loading"
        />
      </div>
      <q-toggle
        v-if="form.web_search_mode !== 'disabled'"
        v-model="form.web_search_citations_enabled"
        :label="t('profile.ai.webSearch.citations')"
        :disable="loading"
      />
    </div>

    <q-toggle v-model="form.enabled" :label="t('profile.ai.enabled')" :disable="loading" />
    <div class="row q-gutter-sm">
      <q-btn
        unelevated
        no-caps
        color="primary"
        text-color="dark"
        icon="mdi-content-save-outline"
        :label="t(scope === 'personal' ? 'profile.ai.save' : 'project.ai.save')"
        :loading="loading"
        :disable="!obviousFieldsReady"
        @click="submit"
      />
      <q-btn
        v-if="settings?.configured"
        flat
        no-caps
        color="negative"
        icon="mdi-delete-outline"
        :label="t(scope === 'personal' ? 'profile.ai.delete' : 'project.ai.delete')"
        :disable="loading"
        @click="emit('delete')"
      />
    </div>
  </div>
</template>

<style scoped>
.ai-settings-form,
.ai-settings-form__section,
.ai-settings-form__control {
  display: grid;
  gap: 14px;
}

.ai-settings-form__grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 14px;
}

.ai-settings-form__wide {
  grid-column: 1 / -1;
}

.ai-settings-form__key {
  border: 1px solid var(--prts-border-soft);
  background: var(--prts-panel-2);
}

.ai-settings-form__section,
.ai-settings-form__section > * {
  min-width: 0;
}

:deep(.ai-settings-form__json) {
  max-height: 220px;
  overflow: auto;
  font-size: 12px;
}

@media (max-width: 640px) {
  .ai-settings-form__grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .ai-settings-form__wide {
    grid-column: auto;
  }

  :deep(.q-btn-toggle) {
    max-width: 100%;
    overflow-x: auto;
  }

  :deep(.q-btn-toggle .q-btn) {
    min-width: max-content;
  }
}
</style>

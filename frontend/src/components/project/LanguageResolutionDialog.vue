<script setup lang="ts">
import { ref, watch } from 'vue'
import { useQuasar } from 'quasar'

import {
  apiErrorMessage,
  projectsApi,
  type ProjectLanguageResolutionDto,
} from '@/api'

const open = defineModel<boolean>({ default: false })
const props = defineProps<{ projectId: number }>()
const emit = defineEmits<{ resolved: [] }>()
const $q = useQuasar()

type IssueSelection = { canonical_tag: string; selected_value: string }

const resolution = ref<ProjectLanguageResolutionDto | null>(null)
const selections = ref<Record<number, IssueSelection>>({})
const loading = ref(false)
const submitting = ref(false)
const sourceLanguages = ref<string[]>([])
const primarySource = ref('')
const targetLanguage = ref('')

/** Load owner-visible raw/canonical metadata only when the dialog opens. */
async function load() {
  loading.value = true
  try {
    const data = await projectsApi.languageResolution(props.projectId)
    resolution.value = data
    sourceLanguages.value = [...data.source_langs]
    primarySource.value = data.primary_source_lang ?? data.source_langs[0] ?? ''
    targetLanguage.value = data.target_lang
    selections.value = Object.fromEntries(
      data.issues.map((issue) => [
        issue.id,
        {
          canonical_tag: issue.canonical_tag ?? issue.raw_tag ?? '',
          selected_value: issue.current_values[0] ?? '',
        },
      ]),
    )
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    open.value = false
  } finally {
    loading.value = false
  }
}

async function submit() {
  if (!resolution.value || !primarySource.value || !targetLanguage.value) return
  submitting.value = true
  try {
    await projectsApi.resolveLanguages(props.projectId, {
      source_langs: sourceLanguages.value,
      primary_source_lang: primarySource.value,
      target_lang: targetLanguage.value,
      issues: resolution.value.issues.map((issue) => {
        if (issue.current_values.length === 0) return { issue_id: issue.id }
        return {
          issue_id: issue.id,
          canonical_tag: selections.value[issue.id]?.canonical_tag,
          selected_value: selections.value[issue.id]?.selected_value,
        }
      }),
    })
    open.value = false
    emit('resolved')
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    submitting.value = false
  }
}

watch(open, (visible) => {
  if (visible) void load()
})
</script>

<template>
  <q-dialog v-model="open" persistent>
    <q-card class="language-resolution">
      <q-card-section>
        <div class="prts-label">{{ $t('project.language.resolutionLabel') }}</div>
        <div class="prts-h2 q-mt-xs">{{ $t('project.language.resolutionHeading') }}</div>
        <p class="prts-dim">{{ $t('project.language.resolutionDescription') }}</p>
      </q-card-section>
      <q-linear-progress v-if="loading" indeterminate />
      <template v-else-if="resolution">
        <q-card-section class="language-resolution__settings">
          <q-select
            v-model="sourceLanguages"
            outlined
            multiple
            use-chips
            use-input
            new-value-mode="add-unique"
            :options="sourceLanguages"
            :label="$t('project.sourceLanguages')"
          />
          <q-select
            v-model="primarySource"
            outlined
            emit-value
            :options="sourceLanguages"
            :label="$t('project.primarySource')"
          />
          <q-input
            v-model="targetLanguage"
            outlined
            :label="$t('project.targetLanguage')"
          />
        </q-card-section>
        <q-separator />
        <q-card-section class="language-resolution__issues">
          <article v-for="issue in resolution.issues" :key="issue.id">
            <div class="row items-center q-gutter-sm">
              <span class="prts-mono">{{ issue.entity_type }} · {{ issue.entity_id }}</span>
              <q-badge outline color="warning" :label="issue.issue_kind" />
            </div>
            <div class="prts-dim q-mt-xs">
              {{ issue.raw_tag ?? '—' }} → {{ issue.canonical_tag ?? '?' }}
            </div>
            <div v-if="issue.current_values.length" class="language-resolution__choice">
              <q-input
                v-model="selections[issue.id]!.canonical_tag"
                outlined
                dense
                :label="$t('project.language.canonicalTag')"
              />
              <q-select
                v-model="selections[issue.id]!.selected_value"
                outlined
                dense
                :options="issue.current_values"
                :label="$t('project.language.selectedValue')"
              />
            </div>
          </article>
        </q-card-section>
      </template>
      <q-card-actions align="right">
        <q-btn v-close-popup flat no-caps :label="$t('project.cancel')" />
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :label="$t('project.language.resolveAction')"
          :loading="submitting"
          :disable="loading"
          @click="submit"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.language-resolution {
  width: 760px;
  max-width: 94vw;
}

.language-resolution__settings,
.language-resolution__choice {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.language-resolution__settings > :first-child {
  grid-column: 1 / -1;
}

.language-resolution__issues {
  display: grid;
  max-height: 42vh;
  gap: 12px;
  overflow: auto;
}

.language-resolution__issues article {
  padding: 12px;
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
}

.language-resolution__choice {
  margin-top: 10px;
}
</style>

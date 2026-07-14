<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import {
  apiErrorMessage,
  posApi,
  termsApi,
  type PosDto,
  type TermDto,
  type TermScope,
  type TermWriteRequest,
} from '@/api'
import TermImportDialog from '@/components/terms/TermImportDialog.vue'
import { hasProjectCapability } from '@/lib/capabilities'
import { displayPosName, type TerminologyDocumentFormat } from '@/lib/terminology'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId } = useProjectWorkspace()
const $q = useQuasar()
const { t, locale } = useI18n()
const terms = ref<TermDto[]>([])
const presets = ref<PosDto[]>([])
const scope = ref<TermScope>('current')
const nextAfter = ref<number | null>(null)
const loading = ref(false)
const loaded = ref(false)
const saving = ref(false)
const exporting = ref(false)
const importOpen = ref(false)
const editOpen = ref(false)
const editingId = ref<number | null>(null)
const form = ref<TermWriteRequest>(emptyForm())
let loadRequest = 0

const canManage = computed(() =>
  hasProjectCapability(detail.value?.capabilities, 'manage_terms'),
)
const canExport = computed(() => hasProjectCapability(detail.value?.capabilities, 'download'))
const scopeOptions = computed(() => [
  { label: t('terminology.scopes.current'), value: 'current' },
  { label: t('terminology.scopes.archived'), value: 'archived' },
  { label: t('terminology.scopes.mixed'), value: 'mixed' },
])
const posOptions = computed(() =>
  presets.value.map((preset) => ({
    label: displayPosName(preset, locale.value) || `#${preset.id}`,
    value: preset.id,
  })),
)

function emptyForm(): TermWriteRequest {
  return {
    source_lang: detail.value?.project.primary_source_lang ?? '',
    source_text: '',
    translation: '',
    notes: '',
    pos_id: null,
    archived: false,
  }
}

async function load(reset = false) {
  if (loading.value && !reset) return
  const request = ++loadRequest
  if (reset) {
    terms.value = []
    nextAfter.value = null
    loaded.value = false
  }
  loading.value = true
  try {
    const page = await termsApi.list(projectId.value, {
      scope: scope.value,
      after: reset ? undefined : (nextAfter.value ?? undefined),
      limit: 50,
    })
    if (request !== loadRequest) return
    terms.value = reset ? page.items : [...terms.value, ...page.items]
    nextAfter.value = page.next_after
    loaded.value = true
  } catch (error) {
    if (request !== loadRequest) return
    $q.notify({
      type: 'negative',
      message: apiErrorMessage(error, t('terminology.loadFailed')),
    })
  } finally {
    if (request === loadRequest) loading.value = false
  }
}

async function loadPresets() {
  try {
    presets.value = await posApi.list()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.pos.loadFailed')) })
  }
}

function openCreate() {
  editingId.value = null
  form.value = emptyForm()
  editOpen.value = true
}

function openEdit(term: TermDto) {
  editingId.value = term.id
  form.value = termRequest(term)
  editOpen.value = true
}

async function saveTerm() {
  saving.value = true
  try {
    if (editingId.value == null) {
      await termsApi.create(projectId.value, form.value)
    } else {
      await termsApi.update(projectId.value, editingId.value, form.value)
    }
    $q.notify({ type: 'positive', message: t('terminology.saved') })
    editOpen.value = false
    await load(true)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.saveFailed')) })
  } finally {
    saving.value = false
  }
}

async function setArchived(term: TermDto, archived: boolean) {
  try {
    await termsApi.update(projectId.value, term.id, { ...termRequest(term), archived })
    $q.notify({
      type: 'positive',
      message: archived ? t('terminology.archived') : t('terminology.restored'),
    })
    await load(true)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.saveFailed')) })
  }
}

function confirmDelete(term: TermDto) {
  $q.dialog({
    title: t('terminology.delete'),
    message: t('terminology.deleteConfirm'),
    cancel: true,
    persistent: true,
  }).onOk(async () => {
    try {
      await termsApi.remove(projectId.value, term.id)
      $q.notify({ type: 'positive', message: t('terminology.deleted') })
      await load(true)
    } catch (error) {
      $q.notify({
        type: 'negative',
        message: apiErrorMessage(error, t('terminology.deleteFailed')),
      })
    }
  })
}

function termRequest(term: TermDto): TermWriteRequest {
  return {
    source_lang: term.source_lang,
    source_text: term.source_text,
    translation: term.translation,
    notes: term.notes,
    pos_id: term.pos_id,
    archived: term.archived,
  }
}

function termPosName(term: TermDto): string {
  return displayPosName(
    { name_zh_cn: term.pos_name_zh_cn, name_en: term.pos_name_en },
    locale.value,
  )
}

function canRestore(term: TermDto): boolean {
  return term.source_lang === detail.value?.project.primary_source_lang
}

async function exportTerms(format: TerminologyDocumentFormat) {
  exporting.value = true
  try {
    const blob = await termsApi.export(projectId.value, format)
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `${detail.value?.project.slug ?? 'project'}-terms.${format}`
    anchor.click()
    URL.revokeObjectURL(url)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.exportFailed')) })
  } finally {
    exporting.value = false
  }
}

onMounted(() => {
  void Promise.all([load(true), loadPresets()])
})
watch(projectId, () => {
  void Promise.all([load(true), loadPresets()])
})
watch(scope, () => load(true))
</script>

<template>
  <section class="terms-view">
    <header class="terms-view__heading">
      <div>
        <div class="prts-label">{{ $t('project.sections.terms') }}</div>
        <h2>{{ $t('terminology.heading') }}</h2>
        <p>{{ $t('terminology.description') }}</p>
      </div>
      <div class="terms-view__actions">
        <q-btn-dropdown
          v-if="canExport"
          outline
          no-caps
          icon="mdi-download-outline"
          :label="$t('terminology.export')"
          :loading="exporting"
        >
          <q-list>
            <q-item v-close-popup clickable @click="exportTerms('csv')">
              <q-item-section>CSV</q-item-section>
            </q-item>
            <q-item v-close-popup clickable @click="exportTerms('json')">
              <q-item-section>JSON</q-item-section>
            </q-item>
          </q-list>
        </q-btn-dropdown>
        <q-btn
          v-if="canManage"
          outline
          no-caps
          icon="mdi-file-import-outline"
          :label="$t('terminology.import.action')"
          @click="importOpen = true"
        />
        <q-btn
          v-if="canManage"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-book-plus-outline"
          :label="$t('terminology.create')"
          @click="openCreate"
        />
      </div>
    </header>

    <q-btn-toggle
      v-model="scope"
      no-caps
      unelevated
      toggle-color="primary"
      toggle-text-color="dark"
      color="grey-9"
      :options="scopeOptions"
      class="terms-view__scope"
    />

    <q-skeleton v-if="!loaded && loading" height="240px" square />
    <div v-else-if="terms.length === 0" class="prts-empty terms-view__empty">
      <q-icon name="mdi-book-alphabet" size="36px" />
      <strong>{{ $t('terminology.empty') }}</strong>
      <span>{{ $t('terminology.emptyHint') }}</span>
    </div>
    <div v-else class="terms-view__table-wrap">
      <q-markup-table flat bordered separator="horizontal">
        <thead>
          <tr>
            <th>{{ $t('terminology.fields.sourceLang') }}</th>
            <th>{{ $t('terminology.fields.sourceText') }}</th>
            <th>{{ $t('terminology.fields.translation') }}</th>
            <th>{{ $t('terminology.fields.pos') }}</th>
            <th>{{ $t('terminology.fields.notes') }}</th>
            <th>{{ $t('terminology.fields.status') }}</th>
            <th v-if="canManage" class="text-right">{{ $t('terminology.fields.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="term in terms" :key="term.id">
            <td class="prts-mono">{{ term.source_lang }}</td>
            <td class="terms-view__content">{{ term.source_text }}</td>
            <td class="terms-view__content">{{ term.translation }}</td>
            <td>{{ termPosName(term) || '—' }}</td>
            <td class="terms-view__notes">{{ term.notes || '—' }}</td>
            <td>
              <q-badge :color="term.archived ? 'grey' : 'positive'" outline>
                {{
                  term.archived
                    ? $t('terminology.scopes.archived')
                    : $t('terminology.active')
                }}
              </q-badge>
            </td>
            <td v-if="canManage" class="text-right terms-view__row-actions">
              <q-btn flat round dense icon="mdi-pencil-outline" @click="openEdit(term)">
                <q-tooltip>{{ $t('terminology.edit') }}</q-tooltip>
              </q-btn>
              <q-btn
                flat
                round
                dense
                :icon="term.archived ? 'mdi-archive-arrow-up-outline' : 'mdi-archive-outline'"
                :disable="term.archived && !canRestore(term)"
                @click="setArchived(term, !term.archived)"
              >
                <q-tooltip>
                  {{
                    term.archived && !canRestore(term)
                      ? $t('terminology.restorePrimaryOnly')
                      : term.archived
                        ? $t('terminology.restore')
                        : $t('terminology.archive')
                  }}
                </q-tooltip>
              </q-btn>
              <q-btn
                flat
                round
                dense
                color="negative"
                icon="mdi-delete-outline"
                @click="confirmDelete(term)"
              >
                <q-tooltip>{{ $t('terminology.delete') }}</q-tooltip>
              </q-btn>
            </td>
          </tr>
        </tbody>
      </q-markup-table>
    </div>

    <div v-if="nextAfter !== null" class="row justify-center">
      <q-btn
        outline
        no-caps
        :label="$t('terminology.loadMore')"
        :loading="loading"
        @click="load()"
      />
    </div>

    <q-dialog v-model="editOpen" persistent>
      <q-card class="terms-view__dialog">
        <q-card-section>
          <div class="prts-label">{{ $t('project.sections.terms') }}</div>
          <div class="prts-h2">
            {{ editingId == null ? $t('terminology.create') : $t('terminology.edit') }}
          </div>
        </q-card-section>
        <q-card-section class="terms-view__form">
          <q-input
            v-model="form.source_lang"
            outlined
            dense
            :label="$t('terminology.fields.sourceLang')"
            :hint="$t('terminology.sourceLangHint')"
          />
          <q-input
            v-model="form.source_text"
            outlined
            autogrow
            :label="$t('terminology.fields.sourceText')"
          />
          <q-input
            v-model="form.translation"
            outlined
            autogrow
            :label="$t('terminology.fields.translation')"
          />
          <q-select
            v-model="form.pos_id"
            outlined
            dense
            clearable
            emit-value
            map-options
            :options="posOptions"
            :label="$t('terminology.fields.pos')"
          />
          <q-input
            v-model="form.notes"
            outlined
            autogrow
            :label="$t('terminology.fields.notes')"
          />
          <q-toggle
            v-model="form.archived"
            :label="$t('terminology.fields.archived')"
            :disable="
              form.archived && form.source_lang !== detail?.project.primary_source_lang
            "
          />
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps :label="$t('project.cancel')" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :label="$t('project.save')"
            :loading="saving"
            :disable="!form.source_lang.trim() || !form.source_text.trim()"
            @click="saveTerm"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>

    <TermImportDialog
      v-model="importOpen"
      kind="term"
      :project-id="projectId"
      @confirmed="load(true)"
    />
  </section>
</template>

<style scoped>
.terms-view {
  display: grid;
  gap: 16px;
}

.terms-view__heading {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: end;
  gap: 20px;
}

.terms-view__heading h2,
.terms-view__heading p {
  margin: 0;
}

.terms-view__heading h2 {
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.terms-view__heading p {
  color: var(--prts-text-dim);
}

.terms-view__actions,
.terms-view__row-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.terms-view__scope {
  justify-self: start;
}

.terms-view__empty {
  display: grid;
  justify-items: center;
  gap: 8px;
  border: 1px dashed var(--prts-border);
}

.terms-view__table-wrap {
  overflow-x: auto;
}

.terms-view__table-wrap table {
  min-width: 980px;
}

.terms-view__content {
  min-width: 180px;
  white-space: pre-wrap;
}

.terms-view__notes {
  max-width: 240px;
  white-space: pre-wrap;
}

.terms-view__row-actions {
  justify-content: flex-end;
  white-space: nowrap;
}

.terms-view__dialog {
  width: min(680px, 94vw);
  max-width: 680px;
}

.terms-view__form {
  display: grid;
  gap: 12px;
}

@media (max-width: 760px) {
  .terms-view__heading {
    grid-template-columns: 1fr;
  }

  .terms-view__actions {
    flex-wrap: wrap;
  }
}
</style>

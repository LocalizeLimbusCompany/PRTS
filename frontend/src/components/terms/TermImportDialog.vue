<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import {
  apiErrorMessage,
  posApi,
  termsApi,
  type ImportPreviewDto,
  type PosPreviewRowDto,
  type TermPreviewRowDto,
} from '@/api'
import { importFormatFromFileName } from '@/lib/terminology'

const props = defineProps<{
  modelValue: boolean
  kind: 'term' | 'pos'
  projectId?: number
}>()
const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  confirmed: []
}>()

const $q = useQuasar()
const { t } = useI18n()
const file = ref<File | null>(null)
const preview = ref<ImportPreviewDto<TermPreviewRowDto | PosPreviewRowDto> | null>(null)
const previewing = ref(false)
const confirming = ref(false)
const open = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
})
const shownRows = computed(() => preview.value?.rows.slice(0, 25) ?? [])

watch(file, () => {
  preview.value = null
})
watch(open, (value) => {
  if (!value) {
    file.value = null
    preview.value = null
  }
})

async function createPreview() {
  if (!file.value) return
  const format = importFormatFromFileName(file.value.name)
  if (!format) {
    $q.notify({ type: 'negative', message: t('terminology.import.invalidFile') })
    return
  }
  previewing.value = true
  try {
    const content = await file.value.text()
    preview.value =
      props.kind === 'term'
        ? await termsApi.previewImport(requiredProjectId(), format, content)
        : await posApi.previewImport(format, content)
  } catch (error) {
    $q.notify({
      type: 'negative',
      message: apiErrorMessage(error, t('terminology.import.previewFailed')),
    })
  } finally {
    previewing.value = false
  }
}

async function confirmImport() {
  if (!preview.value) return
  confirming.value = true
  try {
    const result =
      props.kind === 'term'
        ? await termsApi.confirmImport(
            requiredProjectId(),
            preview.value.token,
            preview.value.digest,
          )
        : await posApi.confirmImport(preview.value.token, preview.value.digest)
    $q.notify({
      type: 'positive',
      message: t('terminology.import.confirmed', {
        created: result.created,
        updated: result.updated,
      }),
    })
    emit('confirmed')
    open.value = false
  } catch (error) {
    $q.notify({
      type: 'negative',
      message: apiErrorMessage(error, t('terminology.import.confirmFailed')),
    })
  } finally {
    confirming.value = false
  }
}

function requiredProjectId(): number {
  if (props.projectId == null) throw new Error('Project ID is required for term import')
  return props.projectId
}

function isTermRow(row: TermPreviewRowDto | PosPreviewRowDto): row is TermPreviewRowDto {
  return 'source_lang' in row
}
</script>

<template>
  <q-dialog v-model="open" persistent>
    <q-card class="term-import-dialog">
      <q-card-section>
        <div class="prts-label">{{ $t('terminology.import.label') }}</div>
        <div class="prts-h2">
          {{
            kind === 'term'
              ? $t('terminology.import.termHeading')
              : $t('terminology.import.posHeading')
          }}
        </div>
        <p class="prts-dim q-mb-none">{{ $t('terminology.import.description') }}</p>
      </q-card-section>

      <q-separator />
      <q-card-section class="term-import-dialog__body">
        <q-file
          v-model="file"
          outlined
          dense
          accept=".csv,.json,text/csv,application/json"
          :label="$t('terminology.import.chooseFile')"
          :disable="previewing || confirming"
        >
          <template #prepend><q-icon name="mdi-file-delimited-outline" /></template>
        </q-file>
        <div class="prts-dim term-import-dialog__format">
          {{
            kind === 'term'
              ? $t('terminology.import.termFields')
              : $t('terminology.import.posFields')
          }}
        </div>

        <template v-if="preview">
          <div class="term-import-dialog__summary">
            <q-badge color="positive" outline>
              {{ $t('terminology.import.created', { count: preview.created }) }}
            </q-badge>
            <q-badge color="info" outline>
              {{ $t('terminology.import.updated', { count: preview.updated }) }}
            </q-badge>
            <q-badge v-if="preview.warnings.length" color="warning" outline>
              {{ $t('terminology.import.warnings', { count: preview.warnings.length }) }}
            </q-badge>
          </div>

          <q-banner
            v-for="warning in preview.warnings"
            :key="`${warning.row}-${warning.code}`"
            dense
            class="term-import-dialog__warning"
          >
            {{
              $t(`terminology.import.warningCodes.${warning.code}`, {
                row: warning.row,
              })
            }}
          </q-banner>

          <div class="term-import-dialog__table-wrap">
            <q-markup-table flat bordered dense separator="cell">
              <thead>
                <tr v-if="kind === 'term'">
                  <th>#</th>
                  <th>{{ $t('terminology.fields.sourceLang') }}</th>
                  <th>{{ $t('terminology.fields.sourceText') }}</th>
                  <th>{{ $t('terminology.fields.matchMode') }}</th>
                  <th>{{ $t('terminology.fields.translation') }}</th>
                  <th>{{ $t('terminology.fields.pos') }}</th>
                  <th>{{ $t('terminology.fields.archived') }}</th>
                  <th>{{ $t('terminology.import.action') }}</th>
                </tr>
                <tr v-else>
                  <th>#</th>
                  <th>{{ $t('terminology.pos.nameZh') }}</th>
                  <th>{{ $t('terminology.pos.nameEn') }}</th>
                  <th>{{ $t('terminology.pos.sortOrder') }}</th>
                  <th>{{ $t('terminology.import.action') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in shownRows" :key="row.row">
                  <template v-if="isTermRow(row)">
                    <td>{{ row.row }}</td>
                    <td class="prts-mono">{{ row.source_lang }}</td>
                    <td>{{ row.source_text }}</td>
                    <td>{{ $t(`terminology.matchModes.${row.match_mode}`) }}</td>
                    <td>{{ row.translation }}</td>
                    <td>{{ row.pos || '—' }}</td>
                    <td>{{ row.archived ? $t('common.yes') : $t('common.no') }}</td>
                  </template>
                  <template v-else>
                    <td>{{ row.row }}</td>
                    <td>{{ row.name_zh_cn || '—' }}</td>
                    <td>{{ row.name_en || '—' }}</td>
                    <td>{{ row.sort_order }}</td>
                  </template>
                  <td>{{ $t(`terminology.import.actions.${row.action}`) }}</td>
                </tr>
              </tbody>
            </q-markup-table>
          </div>
          <div v-if="preview.rows.length > shownRows.length" class="prts-dim">
            {{ $t('terminology.import.previewLimited', { count: shownRows.length }) }}
          </div>
        </template>
      </q-card-section>

      <q-separator />
      <q-card-actions align="right">
        <q-btn v-close-popup flat no-caps :label="$t('project.cancel')" :disable="confirming" />
        <q-btn
          v-if="!preview"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :label="$t('terminology.import.preview')"
          :disable="!file"
          :loading="previewing"
          @click="createPreview"
        />
        <q-btn
          v-else
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :label="$t('terminology.import.confirm')"
          :loading="confirming"
          @click="confirmImport"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.term-import-dialog {
  width: min(960px, 94vw);
  max-width: 960px;
}

.term-import-dialog__body {
  display: grid;
  gap: 12px;
  max-height: 66vh;
  overflow: auto;
}

.term-import-dialog__format {
  font: 11px var(--font-mono);
}

.term-import-dialog__summary {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.term-import-dialog__warning {
  border: 1px solid var(--q-warning);
  color: var(--q-warning);
  background: transparent;
}

.term-import-dialog__table-wrap {
  overflow: auto;
}

.term-import-dialog__table-wrap table {
  min-width: 760px;
}
</style>

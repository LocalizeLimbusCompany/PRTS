<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import {
  apiErrorMessage,
  entriesApi,
  pokeApi,
  projectsApi,
  searchApi,
  suggestionsApi,
  termsApi,
  type EntryDto,
  type EntryState,
  type EntryVersionDto,
  type FileDto,
  type MemberDto,
  type ProjectCapabilities,
  type ProjectDto,
  type SearchHitDto,
  type StructuredSearchRequest,
  type SuggestionDto,
  type TermDto,
} from '@/api'
import SearchFilters from '@/components/SearchFilters.vue'
import SuggestionsPanel from '@/components/SuggestionsPanel.vue'
import EntryCommentsTab from '@/components/editor/EntryCommentsTab.vue'
import EntryHistoryTab from '@/components/editor/EntryHistoryTab.vue'
import EntryTermsTab from '@/components/editor/EntryTermsTab.vue'
import SourceTermText from '@/components/editor/SourceTermText.vue'
import {
  useRealtime,
  canOpenPresenceMenu,
  shouldConnectProjectRealtime,
} from '@/composables/useRealtime'
import { computeSaveButton } from '@/lib/saveButton'
import { STATE_ORDER, stateLabel } from '@/lib/states'
import { insertTermTranslation } from '@/lib/terminology'
import { useAuthStore } from '@/stores/auth'

const props = defineProps<{ id: number }>()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const $q = useQuasar()
const { t } = useI18n()

const project = ref<ProjectDto | null>(null)
const capabilities = ref<ProjectCapabilities | null>(null)
const files = ref<FileDto[]>([])
const members = ref<MemberDto[]>([])
const isNarrow = computed(() => $q.screen.lt.md)
const mobileSection = ref<'list' | 'editor' | 'context'>('list')

const canReview = computed(() => capabilities.value?.review_entry === true)
const canEditLocked = computed(() => capabilities.value?.edit_locked_entry === true)
const canLock = computed(() => capabilities.value?.lock_entry === true)
const canHide = computed(() => capabilities.value?.hide_entry === true)
const canEdit = computed(() => capabilities.value?.edit_entry === true)
const canForcePresence = computed(() => capabilities.value?.force_save_presence === true)
const stateOptions = computed(() =>
  STATE_ORDER.filter((state) => state !== 'questioned').map((state) => ({
    label: stateLabel(state, t),
    value: state,
    disable: ['checked', 'reviewed'].includes(state) ? !canReview.value : !canEdit.value,
  })),
)

const currentFileId = ref<number | null>(route.query.file ? Number(route.query.file) : null)
const currentTaskId = ref<number | null>(route.query.task ? Number(route.query.task) : null)
const isTaskScope = computed(() => currentTaskId.value !== null)
const includeHidden = ref(false)
const fileOptions = computed(() => [
  { label: t('editor.scopeAll'), value: null as number | null },
  ...files.value.map((file) => ({ label: file.path, value: file.id })),
])

const activeSearchRequest = ref<StructuredSearchRequest | null>(null)
const isSearchMode = computed(() => activeSearchRequest.value !== null)
const entries = ref<(EntryDto | SearchHitDto)[]>([])
const listLoading = ref(false)
const PAGE_SIZES = [50, 100, 200] as const
const storedPageSize = Number(localStorage.getItem('prts_editor_page_size'))
const pageSize = ref<number>(
  PAGE_SIZES.includes(storedPageSize as (typeof PAGE_SIZES)[number]) ? storedPageSize : 100,
)
const currentPage = ref(1)
const totalItems = ref(0)
const browseCursors = ref<Array<number | null>>([null])
const searchCursors = ref<Array<string | null>>([null])
const totalPages = computed(() => Math.ceil(totalItems.value / pageSize.value))
const hasPreviousPage = computed(() => currentPage.value > 1)
const hasNextPage = computed(() => currentPage.value < totalPages.value)
let listLoadGeneration = 0

function isHit(entry: EntryDto | SearchHitDto): entry is SearchHitDto {
  return 'rrf_score' in entry
}

function relevancePct(score: number): number {
  return Math.min(99, Math.round(score * 100))
}

function browseParams(after?: number) {
  return {
    file_id: currentFileId.value ?? undefined,
    task_id: currentTaskId.value ?? undefined,
    after,
    limit: pageSize.value,
    include_hidden: includeHidden.value,
  }
}

async function resetAndLoad() {
  currentPage.value = 1
  browseCursors.value = [null]
  searchCursors.value = [null]
  await loadPage(1)
}

async function loadPage(page: number) {
  if (page < 1) return
  const generation = ++listLoadGeneration
  const searchRequest = activeSearchRequest.value ? { ...activeSearchRequest.value } : null
  const requestedPageSize = pageSize.value
  listLoading.value = true
  try {
    if (searchRequest) {
      const cursor = searchCursors.value[page - 1]
      if (page > 1 && cursor == null) return
      const response = await searchApi.search(props.id, {
        ...searchRequest,
        after: cursor ?? undefined,
        limit: requestedPageSize,
      })
      if (generation !== listLoadGeneration) return
      entries.value = response.items
      totalItems.value = response.total_items
      searchCursors.value[page] = response.next_after
    } else {
      const cursor = browseCursors.value[page - 1]
      if (page > 1 && cursor == null) return
      const params = browseParams(cursor ?? undefined)
      const [items, count] = await Promise.all([
        entriesApi.list(props.id, params),
        entriesApi.count(props.id, {
          file_id: params.file_id,
          task_id: params.task_id,
          include_hidden: params.include_hidden,
        }),
      ])
      if (generation !== listLoadGeneration) return
      entries.value = items
      totalItems.value = count.total_items
      browseCursors.value[page] =
        items.length === requestedPageSize ? (items.at(-1)?.id ?? null) : null
    }
    currentPage.value = page
    if (selected.value && !entries.value.some((entry) => entry.id === selected.value?.id)) {
      clearSelection()
    }
  } catch (error) {
    if (generation === listLoadGeneration) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    }
  } finally {
    if (generation === listLoadGeneration) listLoading.value = false
  }
}

async function goNextPage() {
  if (hasNextPage.value) await loadPage(currentPage.value + 1)
}

async function goPreviousPage() {
  if (hasPreviousPage.value) await loadPage(currentPage.value - 1)
}

async function runSearch(request: StructuredSearchRequest) {
  activeSearchRequest.value = { ...request, after: undefined }
  await resetAndLoad()
}

function onSearchClear() {
  activeSearchRequest.value = null
  void resetAndLoad()
}

watch(currentFileId, () => {
  clearSelection()
  if (!isSearchMode.value) void resetAndLoad()
})
watch(includeHidden, () => {
  if (selected.value?.hidden && !includeHidden.value) clearSelection()
  if (!isSearchMode.value) void resetAndLoad()
})
watch(pageSize, (size) => {
  localStorage.setItem('prts_editor_page_size', String(size))
  void resetAndLoad()
})

const selected = ref<EntryDto | null>(null)
const draft = ref('')
const draftState = ref<EntryState>('untranslated')
const saving = ref(false)
const translationElement = ref<HTMLTextAreaElement | null>(null)
const suggestions = ref<SuggestionDto[]>([])
const matchedTerms = ref<TermDto[]>([])
const history = ref<EntryVersionDto[]>([])
const contextTab = ref<'terms' | 'history' | 'comments'>('terms')
const commentsRefreshToken = ref(0)
const questionDialog = ref(false)
const questionReason = ref('')
let contextLoadGeneration = 0
let historyLoadGeneration = 0

const panelReadOnly = computed(
  () => !canEdit.value || (selected.value?.locked === true && !canEditLocked.value),
)
const translationDirty = computed(
  () => !!selected.value && draft.value !== selected.value.translation,
)
const stateChanged = computed(() => !!selected.value && draftState.value !== selected.value.state)
const othersEditingSelected = computed(() =>
  selected.value ? otherEditing(selected.value.id) : false,
)
const saveBtn = computed(() =>
  computeSaveButton({
    canEdit: canEdit.value,
    canReview: canReview.value,
    canEditLocked: canEditLocked.value,
    canForcePresence: canForcePresence.value,
    locked: selected.value?.locked === true,
    state: draftState.value,
    dirty: translationDirty.value,
    stateChanged: stateChanged.value,
    presenceConflict: othersEditingSelected.value,
  }),
)
const sourceLangs = computed(() => project.value?.source_langs ?? [])

function captureTranslationElement(event: Event) {
  translationElement.value = event.target as HTMLTextAreaElement
}

function insertTranslation(translation: string) {
  const element = translationElement.value
  const start = element?.selectionStart ?? draft.value.length
  const end = element?.selectionEnd ?? start
  const inserted = insertTermTranslation(draft.value, start, end, translation)
  draft.value = inserted.value
  void nextTick(() => {
    element?.focus()
    element?.setSelectionRange(inserted.cursor, inserted.cursor)
  })
}

async function loadEntryContext(entry: EntryDto) {
  const generation = ++contextLoadGeneration
  const primary = project.value?.primary_source_lang
  const source = primary ? entry.original[primary] : undefined
  // History has its own generation guard because a save can refresh it while the other context
  // requests for the selected entry are still in flight.
  void refreshEntryHistory(entry.id)
  const [tm, terms] = await Promise.allSettled([
    suggestionsApi.forEntry(props.id, entry.id),
    primary && source ? termsApi.match(props.id, source, 5_000) : Promise.resolve([]),
  ])
  if (generation !== contextLoadGeneration || selected.value?.id !== entry.id) return
  suggestions.value = tm.status === 'fulfilled' ? tm.value : []
  matchedTerms.value =
    terms.status === 'fulfilled'
      ? terms.value.filter(
          (term) => !term.archived && !term.deleted && term.source_lang === primary,
        )
      : []
}

/** Reload the authoritative newest-first entry history without letting stale requests win. */
async function refreshEntryHistory(entryId: number) {
  const generation = ++historyLoadGeneration
  try {
    const versions = await entriesApi.history(props.id, entryId)
    if (generation !== historyLoadGeneration || selected.value?.id !== entryId) return
    history.value = versions
  } catch {
    // History is contextual data: a failed refresh must not turn an already committed save into
    // a reported save failure. A later selection, save, or realtime update will retry the read.
  }
}

function select(entry: EntryDto | SearchHitDto) {
  selected.value = entry
  draft.value = entry.translation
  draftState.value = entry.state
  history.value = []
  sendEditing(entry.id)
  if (isNarrow.value) mobileSection.value = 'editor'
  void loadEntryContext(entry)
}

function clearSelection() {
  contextLoadGeneration += 1
  historyLoadGeneration += 1
  selected.value = null
  draft.value = ''
  suggestions.value = []
  matchedTerms.value = []
  history.value = []
  if (currentFileId.value != null) sendViewing(currentFileId.value)
  else sendIdle()
}

async function persist(targetState: EntryState, questionReasonValue?: string): Promise<boolean> {
  if (!selected.value) return false
  const entryId = selected.value.id
  const version = selected.value.version
  const translation = draft.value
  const forcePresence = saveBtn.value.mode === 'force'
  saving.value = true
  try {
    const updated = await entriesApi.update(props.id, entryId, {
      translation,
      state: targetState,
      version,
      force_presence: forcePresence,
      question_reason: questionReasonValue || undefined,
    })
    if (selected.value?.id === entryId) draftState.value = targetState
    applyUpdated(updated)
    if (selected.value?.id === entryId) await refreshEntryHistory(entryId)
    if (questionReasonValue) commentsRefreshToken.value += 1
    $q.notify({ type: 'positive', message: t('editor.saved'), timeout: 900 })
    return true
  } catch (error) {
    const status = (error as { response?: { status?: number } }).response?.status
    if (status === 409) {
      $q.notify({ type: 'warning', message: t('editor.versionConflict') })
      const fresh = await entriesApi.get(props.id, entryId)
      applyUpdated(fresh)
      if (selected.value?.id === entryId) select(fresh)
    } else {
      $q.notify({ type: 'negative', message: apiErrorMessage(error, t('editor.saveFailed')) })
    }
    return false
  } finally {
    saving.value = false
  }
}

async function save() {
  if (!selected.value || saveBtn.value.disabled) return
  await persist(saveBtn.value.targetState ?? draftState.value)
}

async function markQuestioned() {
  if (await persist('questioned', questionReason.value.trim() || undefined)) {
    questionDialog.value = false
    questionReason.value = ''
  }
}

function applyUpdated(updated: EntryDto) {
  const index = entries.value.findIndex((entry) => entry.id === updated.id)
  if (index >= 0) entries.value[index] = updated
  if (selected.value?.id === updated.id) selected.value = updated
}

async function toggleFlag(flag: 'locked' | 'hidden') {
  if (!selected.value) return
  try {
    const body =
      flag === 'locked' ? { locked: !selected.value.locked } : { hidden: !selected.value.hidden }
    const updated = await entriesApi.setFlags(props.id, selected.value.id, body)
    applyUpdated(updated)
    if (flag === 'hidden' && updated.hidden && !includeHidden.value) {
      clearSelection()
      await resetAndLoad()
    } else if (selected.value?.id === updated.id) {
      await refreshEntryHistory(updated.id)
    }
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

function handleRemoteUpdate(entryId: number, _version: number, by: number) {
  if (by === auth.user?.id) return
  if (entries.value.some((entry) => entry.id === entryId) || selected.value?.id === entryId) {
    entriesApi
      .get(props.id, entryId)
      .then((fresh) => {
        const currentViewIncludesHidden =
          activeSearchRequest.value?.include_hidden ?? includeHidden.value
        if (fresh.hidden && !currentViewIncludesHidden) {
          if (selected.value?.id === fresh.id) clearSelection()
          void resetAndLoad()
        } else {
          applyUpdated(fresh)
          if (selected.value?.id === fresh.id) void refreshEntryHistory(fresh.id)
        }
      })
      .catch(() => {})
  }
}

const {
  presences,
  editing: editingMap,
  sendEditing,
  sendViewing,
  sendIdle,
} = useRealtime(
  () => props.id,
  {
    onEntryUpdated: handleRemoteUpdate,
    onEntryCommentChanged: (entryId, by) => {
      if (entryId === selected.value?.id && by !== auth.user?.id) commentsRefreshToken.value += 1
    },
  },
  () => shouldConnectProjectRealtime(auth.isAuthed, capabilities.value?.collaborate === true),
)

interface PresenceUser {
  user_id: number
  username: string
  avatar_url: string | null
}
function presenceUser(userId: number): PresenceUser {
  const member = members.value.find((candidate) => candidate.user_id === userId)
  if (member) return member
  if (auth.user?.id === userId)
    return { user_id: userId, username: auth.user.username, avatar_url: auth.user.avatar_url }
  return { user_id: userId, username: `#${userId}`, avatar_url: null }
}
function editorsOf(entryId: number): PresenceUser[] {
  return (editingMap.value[entryId] ?? []).map(presenceUser)
}
function otherEditing(entryId: number): boolean {
  return (editingMap.value[entryId] ?? []).some((userId) => userId !== auth.user?.id)
}
function presenceMenuAllowed(target: PresenceUser): boolean {
  return canOpenPresenceMenu(
    target.user_id,
    auth.user?.id,
    capabilities.value?.collaborate === true,
  )
}
const activeFileId = computed(() => selected.value?.file_id ?? currentFileId.value)
const fileOnlineUsers = computed(() => {
  const ids = new Set(
    presences.value
      .filter((presence) => presence.file_id === activeFileId.value)
      .map((presence) => presence.user_id),
  )
  return [...ids].map(presenceUser)
})

const pokeText = ref('')
const pokeSending = ref(false)
function openDm(target: PresenceUser) {
  if (presenceMenuAllowed(target))
    void router.push({ name: 'message-thread', params: { userId: target.user_id } })
}
async function sendPoke(target: PresenceUser) {
  if (!presenceMenuAllowed(target) || !pokeText.value.trim()) return
  pokeSending.value = true
  try {
    await pokeApi.send(props.id, target.user_id, pokeText.value.trim())
    pokeText.value = ''
  } finally {
    pokeSending.value = false
  }
}

const leftCollapsed = ref(localStorage.getItem('prts_editor_left_collapsed') === '1')
const rightCollapsed = ref(localStorage.getItem('prts_editor_right_collapsed') === '1')
const leftWidth = ref(Number(localStorage.getItem('prts_editor_left_width') ?? 340))
const rightWidth = ref(Number(localStorage.getItem('prts_editor_right_width') ?? 360))
let stopResize: (() => void) | null = null
function startResize(side: 'left' | 'right', event: PointerEvent) {
  const startX = event.clientX
  const startWidth = side === 'left' ? leftWidth.value : rightWidth.value
  const move = (moveEvent: PointerEvent) => {
    const delta = moveEvent.clientX - startX
    const width = Math.max(260, Math.min(620, startWidth + (side === 'left' ? delta : -delta)))
    if (side === 'left') leftWidth.value = width
    else rightWidth.value = width
  }
  const up = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    localStorage.setItem(
      `prts_editor_${side}_width`,
      String(side === 'left' ? leftWidth.value : rightWidth.value),
    )
    stopResize = null
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
  stopResize = up
}
watch(leftCollapsed, (value) =>
  localStorage.setItem('prts_editor_left_collapsed', value ? '1' : '0'),
)
watch(rightCollapsed, (value) =>
  localStorage.setItem('prts_editor_right_collapsed', value ? '1' : '0'),
)
onBeforeUnmount(() => {
  stopResize?.()
  sendIdle()
})

onMounted(async () => {
  try {
    const [detail, tree, projectMembers] = await Promise.all([
      projectsApi.get(props.id),
      projectsApi.tree(props.id),
      projectsApi.members(props.id),
    ])
    project.value = detail.project
    capabilities.value = detail.capabilities
    files.value = tree.files
    members.value = projectMembers
    if (currentFileId.value != null) sendViewing(currentFileId.value)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
  await resetAndLoad()
})
</script>

<template>
  <q-page class="editor-page">
    <div class="editor-bar">
      <q-btn
        flat
        dense
        round
        icon="mdi-arrow-left"
        :to="{ name: 'project-info', params: { id: props.id } }"
        ><q-tooltip>{{ t('editor.backToProject') }}</q-tooltip></q-btn
      >
      <div class="prts-display ellipsis editor-title">{{ project?.name ?? '…' }}</div>
      <q-select
        v-if="!isTaskScope"
        v-model="currentFileId"
        :options="fileOptions"
        dense
        outlined
        options-dense
        emit-value
        map-options
        class="editor-fileselect"
      />
      <q-chip
        v-else
        dense
        square
        color="primary"
        text-color="dark"
        icon="mdi-clipboard-text-outline"
        >{{ $t('editor.taskScope', { id: currentTaskId }) }}</q-chip
      >
      <q-toggle
        v-if="canHide && !isTaskScope"
        v-model="includeHidden"
        :label="t('editor.includeHidden')"
        dense
      />
      <q-chip
        v-if="isSearchMode"
        dense
        square
        color="secondary"
        text-color="dark"
        icon="mdi-file-search-outline"
        >{{ t('editor.searchMode') }}</q-chip
      >
      <q-space />
      <q-chip
        v-if="activeFileId"
        dense
        square
        clickable
        icon="mdi-account-multiple-outline"
        color="primary"
        text-color="dark"
        class="prts-mono"
      >
        {{ fileOnlineUsers.length }}
        <q-tooltip>{{ t('editor.fileOnlineCount', { count: fileOnlineUsers.length }) }}</q-tooltip>
        <q-menu
          ><q-list dense style="min-width: 220px"
            ><q-item v-for="user in fileOnlineUsers" :key="user.user_id"
              ><q-item-section avatar
                ><q-avatar size="28px" color="primary" text-color="dark"
                  ><img v-if="user.avatar_url" :src="user.avatar_url" alt="" /><span v-else>{{
                    user.username.charAt(0).toUpperCase()
                  }}</span></q-avatar
                ></q-item-section
              ><q-item-section>{{ user.username }}</q-item-section></q-item
            ><q-item v-if="fileOnlineUsers.length === 0"
              ><q-item-section class="prts-dim">{{
                t('editor.noFileOnline')
              }}</q-item-section></q-item
            ></q-list
          ></q-menu
        >
      </q-chip>
      <div v-if="isNarrow" class="row q-gutter-xs">
        <q-btn-toggle
          v-model="mobileSection"
          dense
          no-caps
          unelevated
          toggle-color="primary"
          :options="[
            { label: t('editor.entryList'), value: 'list' },
            { label: t('editor.editArea'), value: 'editor' },
            { label: t('editor.contextArea'), value: 'context' },
          ]"
        />
      </div>
    </div>

    <div class="editor-body">
      <aside
        v-show="(!isNarrow && !leftCollapsed) || (isNarrow && mobileSection === 'list')"
        class="ed-pane ed-pane--list"
        :style="!isNarrow ? { width: `${leftWidth}px` } : undefined"
      >
        <div class="pane-head">
          <span>{{ t('editor.entryList') }}</span
          ><q-btn
            v-if="!isNarrow"
            flat
            round
            dense
            icon="mdi-chevron-left"
            @click="leftCollapsed = true"
          />
        </div>
        <SearchFilters
          :files="files"
          :source-langs="sourceLangs"
          :current-file-id="currentFileId"
          :current-task-id="currentTaskId"
          :can-include-hidden="canHide"
          @search="runSearch"
          @clear="onSearchClear"
        />
        <q-virtual-scroll :items="entries" class="entry-list">
          <template #default="{ item }">
            <div
              class="entry-row"
              :class="{ active: item.id === selected?.id }"
              @click="select(item)"
            >
              <span class="state-dot" :class="`state-${item.state}`" />
              <div class="entry-row__body">
                <div class="entry-row__key prts-mono">{{ item.key }}</div>
                <div class="entry-row__preview">
                  {{ item.translation || Object.values(item.original)[0] || '—' }}
                </div>
              </div>
              <span v-if="isHit(item)" class="relevance-badge"
                >{{ relevancePct(item.rrf_score) }}%</span
              >
              <q-icon v-if="item.locked" name="mdi-lock-outline" size="14px" />
              <q-icon v-if="item.hidden" name="mdi-eye-off-outline" size="14px" />
              <div v-if="editorsOf(item.id).length" class="avatar-stack" @click.stop>
                <q-avatar
                  v-for="editor in editorsOf(item.id)"
                  :key="editor.user_id"
                  size="20px"
                  color="secondary"
                  text-color="dark"
                  class="poke-avatar"
                >
                  <img
                    v-if="editor.avatar_url"
                    :src="editor.avatar_url"
                    :alt="editor.username"
                  /><span v-else>{{ editor.username.charAt(0).toUpperCase() }}</span>
                  <q-tooltip>{{ editor.username }} · {{ t('editor.editingNow') }}</q-tooltip>
                  <q-menu v-if="presenceMenuAllowed(editor)"
                    ><div class="poke-compose">
                      <div class="prts-label q-mb-xs">
                        {{ t('poke.composeTitle', { name: editor.username }) }}
                      </div>
                      <q-input
                        v-model="pokeText"
                        dense
                        outlined
                        maxlength="140"
                        @keyup.enter="sendPoke(editor)"
                      />
                      <div class="row justify-end q-mt-sm q-gutter-xs">
                        <q-btn
                          v-close-popup
                          flat
                          dense
                          no-caps
                          icon="mdi-email-outline"
                          :label="t('dm.entry')"
                          @click="openDm(editor)"
                        /><q-btn
                          v-close-popup
                          unelevated
                          dense
                          no-caps
                          color="primary"
                          text-color="dark"
                          :label="t('poke.send')"
                          :loading="pokeSending"
                          @click="sendPoke(editor)"
                        />
                      </div></div
                  ></q-menu>
                </q-avatar>
              </div>
            </div>
          </template>
        </q-virtual-scroll>
        <div v-if="listLoading" class="row justify-center q-pa-sm">
          <q-spinner color="primary" />
        </div>
        <div v-else-if="entries.length === 0" class="prts-empty">
          {{ t(isSearchMode ? 'editor.noSearchResults' : 'editor.noResults') }}
        </div>
        <div class="pagination-bar">
          <q-select
            v-model="pageSize"
            dense
            outlined
            emit-value
            map-options
            :options="PAGE_SIZES.map((value) => ({ label: String(value), value }))"
          /><q-space /><q-btn
            flat
            round
            dense
            icon="mdi-chevron-left"
            :disable="!hasPreviousPage"
            @click="goPreviousPage"
          /><span class="prts-mono">{{ totalPages ? currentPage : 0 }} / {{ totalPages }}</span
          ><q-btn
            flat
            round
            dense
            icon="mdi-chevron-right"
            :disable="!hasNextPage"
            @click="goNextPage"
          />
        </div>
      </aside>
      <div
        v-if="!isNarrow && !leftCollapsed"
        class="pane-resizer"
        @pointerdown="startResize('left', $event)"
      />
      <q-btn
        v-if="!isNarrow && leftCollapsed"
        flat
        round
        dense
        icon="mdi-chevron-right"
        class="collapsed-control"
        @click="leftCollapsed = false"
        ><q-tooltip>{{ t('editor.expandEntryList') }}</q-tooltip></q-btn
      >

      <main v-show="!isNarrow || mobileSection === 'editor'" class="ed-pane ed-pane--editor">
        <div v-if="!selected" class="prts-empty editor-empty">{{ t('editor.selectFromLeft') }}</div>
        <div v-else class="panel">
          <div class="entry-toolbar">
            <div class="prts-label">KEY</div>
            <div class="prts-mono ellipsis entry-key">{{ selected.key }}</div>
            <q-space /><q-btn
              v-if="canEdit"
              flat
              dense
              round
              size="sm"
              icon="mdi-help-circle-outline"
              :disable="panelReadOnly"
              :color="selected.state === 'questioned' ? 'warning' : undefined"
              @click="questionDialog = true"
              ><q-tooltip>{{ t('editor.markQuestioned') }}</q-tooltip></q-btn
            ><q-btn
              v-if="canLock"
              flat
              dense
              round
              size="sm"
              :icon="selected.locked ? 'mdi-lock' : 'mdi-lock-open-outline'"
              :color="selected.locked ? 'warning' : undefined"
              @click="toggleFlag('locked')"
              ><q-tooltip>{{
                t(selected.locked ? 'editor.unlock' : 'editor.lock')
              }}</q-tooltip></q-btn
            ><q-btn
              v-if="canHide"
              flat
              dense
              round
              size="sm"
              :icon="selected.hidden ? 'mdi-eye-off-outline' : 'mdi-eye-outline'"
              @click="toggleFlag('hidden')"
              ><q-tooltip>{{
                t(selected.hidden ? 'editor.unhide' : 'editor.hide')
              }}</q-tooltip></q-btn
            >
          </div>
          <div class="orig-block">
            <div v-for="lang in sourceLangs" :key="lang" class="orig-row">
              <div class="prts-label orig-lang">{{ lang }}</div>
              <div class="orig-text">
                <SourceTermText
                  v-if="lang === project?.primary_source_lang"
                  :source="selected.original[lang] ?? '—'"
                  :terms="matchedTerms"
                  @apply="insertTranslation"
                /><template v-else>{{ selected.original[lang] ?? '—' }}</template>
              </div>
            </div>
          </div>
          <div class="prts-label q-mt-md q-mb-xs">
            {{ t('editor.translation') }} → {{ project?.target_lang }}
          </div>
          <q-input
            v-model="draft"
            type="textarea"
            outlined
            autogrow
            :readonly="panelReadOnly"
            input-class="prts-translation"
            :input-style="{ minHeight: '150px' }"
            @focus="captureTranslationElement"
          />
          <SuggestionsPanel
            v-if="!panelReadOnly"
            :suggestions="suggestions"
            @apply="draft = $event"
          />
          <div v-if="canEdit" class="row items-center justify-end q-mt-md q-gutter-sm">
            <q-select
              v-model="draftState"
              :options="stateOptions"
              dense
              outlined
              emit-value
              map-options
              :disable="panelReadOnly"
              style="min-width: 140px"
            /><q-btn
              unelevated
              no-caps
              :color="saveBtn.color"
              :text-color="saveBtn.color ? 'dark' : undefined"
              icon="mdi-content-save-outline"
              :label="t(`editor.btn_${saveBtn.labelKey}`)"
              :loading="saving"
              :disable="saveBtn.disabled"
              @click="save"
            />
          </div>
          <div v-else class="prts-dim q-mt-md text-right">{{ t('editor.readOnlyGuest') }}</div>
        </div>
      </main>

      <q-btn
        v-if="!isNarrow && rightCollapsed"
        flat
        round
        dense
        icon="mdi-chevron-left"
        class="collapsed-control"
        @click="rightCollapsed = false"
        ><q-tooltip>{{ t('editor.expandContext') }}</q-tooltip></q-btn
      >
      <div
        v-if="!isNarrow && !rightCollapsed"
        class="pane-resizer"
        @pointerdown="startResize('right', $event)"
      />
      <aside
        v-show="(!isNarrow && !rightCollapsed) || (isNarrow && mobileSection === 'context')"
        class="ed-pane ed-pane--context"
        :style="!isNarrow ? { width: `${rightWidth}px` } : undefined"
      >
        <div class="pane-head">
          <q-btn
            v-if="!isNarrow"
            flat
            round
            dense
            icon="mdi-chevron-right"
            @click="rightCollapsed = true"
          /><q-tabs
            v-model="contextTab"
            dense
            no-caps
            active-color="primary"
            indicator-color="primary"
            class="context-tabs"
            ><q-tab name="terms" icon="mdi-book-alphabet" :label="t('editor.termsTab')" /><q-tab
              name="history"
              icon="mdi-history"
              :label="t('editor.history')" /><q-tab
              name="comments"
              icon="mdi-comment-text-outline"
              :label="t('editor.commentsTab')"
          /></q-tabs>
        </div>
        <div v-if="!selected" class="prts-empty editor-empty">
          {{ t('editor.selectForContext') }}
        </div>
        <q-tab-panels v-else v-model="contextTab" animated class="context-panels"
          ><q-tab-panel name="terms"
            ><EntryTermsTab :terms="matchedTerms" @apply="insertTranslation" /></q-tab-panel
          ><q-tab-panel name="history"
            ><EntryHistoryTab
              :history="history"
              :mode="auth.user?.entry_diff_mode ?? 'word_inline'"
              :primary-source="project?.primary_source_lang ?? null" /></q-tab-panel
          ><q-tab-panel name="comments"
            ><EntryCommentsTab
              :project-id="props.id"
              :entry-id="selected.id"
              :refresh-token="commentsRefreshToken" /></q-tab-panel
        ></q-tab-panels>
      </aside>
    </div>

    <q-dialog v-model="questionDialog"
      ><q-card class="question-card"
        ><q-card-section
          ><div class="prts-h2">{{ t('editor.markQuestioned') }}</div>
          <div class="prts-dim">{{ t('editor.questionReasonHint') }}</div></q-card-section
        ><q-card-section
          ><q-input
            v-model="questionReason"
            outlined
            type="textarea"
            autogrow
            maxlength="4000"
            counter
            :label="t('editor.questionReason')" /></q-card-section
        ><q-card-actions align="right"
          ><q-btn v-close-popup flat no-caps :label="t('common.cancel')" /><q-btn
            unelevated
            no-caps
            color="warning"
            text-color="dark"
            :label="t('editor.confirmQuestioned')"
            :loading="saving"
            @click="markQuestioned" /></q-card-actions></q-card
    ></q-dialog>
  </q-page>
</template>

<style scoped>
.editor-page {
  height: calc(100vh - var(--prts-nav-h));
  display: flex;
  flex-direction: column;
  background: var(--prts-bg);
  background-image: none;
}
.editor-bar {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--prts-border);
  background: var(--prts-panel);
  flex-wrap: wrap;
}
.editor-title {
  max-width: 190px;
  font-size: 14px;
}
.editor-fileselect {
  min-width: 170px;
  max-width: 280px;
}
.editor-body {
  display: flex;
  flex: 1;
  min-height: 0;
  background: var(--prts-bg);
}
.ed-pane {
  min-width: 0;
  height: 100%;
  min-height: 0;
  background: var(--prts-bg);
}
.ed-pane--list,
.ed-pane--context {
  display: flex;
  flex: 0 0 auto;
  flex-direction: column;
}
.ed-pane--editor {
  flex: 1;
  overflow: auto;
}
.ed-pane--list {
  border-right: 1px solid var(--prts-border);
}
.ed-pane--context {
  border-left: 1px solid var(--prts-border);
}
.pane-head {
  display: flex;
  align-items: center;
  min-height: 38px;
  padding: 3px 8px;
  border-bottom: 1px solid var(--prts-border);
  color: var(--prts-text-dim);
}
.pane-resizer {
  z-index: 2;
  flex: 0 0 5px;
  margin: 0 -2px;
  cursor: col-resize;
}
.pane-resizer:hover {
  background: var(--prts-accent);
}
.collapsed-control {
  align-self: flex-start;
  margin: 4px;
}
.entry-list {
  flex: 1;
  min-height: 0;
}
.entry-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 54px;
  padding: 7px 11px;
  border-bottom: 1px solid var(--prts-border-soft);
  cursor: pointer;
}
.entry-row:hover {
  background: var(--prts-panel-2);
}
.entry-row.active {
  background: var(--prts-accent-dim);
  box-shadow: inset 2px 0 var(--prts-accent);
}
.entry-row__body {
  min-width: 0;
  flex: 1;
}
.entry-row__key,
.entry-row__preview {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.entry-row__key {
  color: var(--prts-text-dim);
  font-size: 11px;
}
.entry-row__preview {
  font-size: 13px;
}
.avatar-stack {
  display: flex;
  flex-direction: row-reverse;
  padding-left: 8px;
}
.avatar-stack :deep(.q-avatar) {
  margin-left: -7px;
  border: 1px solid var(--prts-bg);
}
.poke-avatar {
  cursor: pointer;
}
.poke-compose {
  width: 250px;
  padding: 10px;
}
.relevance-badge {
  padding: 1px 4px;
  border: 1px solid var(--prts-border);
  border-radius: 3px;
  color: var(--prts-text-dim);
  font-size: 10px;
}
.pagination-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  min-height: 48px;
  padding: 5px 8px;
  border-top: 1px solid var(--prts-border);
}
.pagination-bar :deep(.q-field) {
  width: 78px;
}
.panel {
  padding: 18px 22px 40px;
}
.entry-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 9px;
}
.entry-key {
  max-width: 55%;
  font-size: 13px;
}
.orig-block {
  overflow: hidden;
  border: 1px solid var(--prts-border);
  border-radius: 3px;
  background: var(--prts-bg-elev);
}
.orig-row {
  display: flex;
  gap: 14px;
  padding: 11px 14px;
  border-bottom: 1px solid var(--prts-border-soft);
}
.orig-row:last-child {
  border-bottom: 0;
}
.orig-lang {
  flex: 0 0 54px;
}
.orig-text {
  color: var(--prts-text-strong);
  line-height: 1.65;
  white-space: pre-wrap;
}
:deep(.prts-translation) {
  font-size: 14px;
  line-height: 1.7;
}
.context-tabs {
  min-width: 0;
  flex: 1;
}
.context-panels {
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: transparent;
}
.context-panels :deep(.q-tab-panel) {
  padding: 0;
}
.editor-empty {
  padding-top: 90px;
}
.question-card {
  width: min(560px, 94vw);
}
@media (max-width: 1023px) {
  .editor-page {
    height: auto;
    min-height: calc(100vh - var(--prts-nav-h));
  }
  .editor-body {
    min-height: calc(100vh - 116px);
  }
  .ed-pane {
    width: 100% !important;
  }
  .panel {
    padding: 14px 12px 32px;
  }
}
</style>

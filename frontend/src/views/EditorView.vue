<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
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
  type EntryDto,
  type EntryVersionDto,
  type FileDto,
  type MemberDto,
  type ProjectDto,
  type SearchHitDto,
  type SuggestionDto,
} from '@/api'
import { STATE_LABELS, STATE_ORDER, stateLabel } from '@/lib/states'
import { useRealtime } from '@/composables/useRealtime'
import { useAuthStore } from '@/stores/auth'
import SearchFilters from '@/components/SearchFilters.vue'
import SuggestionsPanel from '@/components/SuggestionsPanel.vue'
import type { SearchParams } from '@/components/SearchFilters.vue'
import { computeSaveButton } from '@/lib/saveButton'

const props = defineProps<{ id: number }>()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const $q = useQuasar()
const { t } = useI18n()

const isNarrow = computed(() => $q.screen.lt.md)
const mobilePanel = ref(false)

const project = ref<ProjectDto | null>(null)
const files = ref<FileDto[]>([])
const members = ref<MemberDto[]>([])

/* —— 权限 —— */
const myRole = computed(() => {
  if (auth.isAdmin) return 'owner'
  return members.value.find((m) => m.user_id === auth.user?.id)?.role ?? null
})
const isMember = computed(() => myRole.value !== null)
const canReview = computed(() => ['owner', 'manager', 'reviewer'].includes(myRole.value ?? ''))
const canEditLocked = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const canFlag = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const isManager = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const canEdit = computed(() => isMember.value) // 任何成员都有 PROJECT_ENTRY_EDIT
const availableStates = computed(() =>
  canReview.value ? STATE_ORDER : ['untranslated', 'translated', 'questioned'],
)

/* —— 筛选 —— */
const currentFileId = ref<number | null>(route.query.file ? Number(route.query.file) : null)
const currentTaskId = ref<number | null>(route.query.task ? Number(route.query.task) : null)
const isTaskScope = computed(() => currentTaskId.value !== null)
const includeHidden = ref(false)
const fileOptions = computed(() => [
  { label: '全部文件', value: null as number | null },
  ...files.value.map((f) => ({ label: f.path, value: f.id })),
])

/* —— 搜索模式（search）vs 浏览模式（browse）—— */
/** 当前搜索参数；null 表示浏览模式。 */
const activeSearchParams = ref<SearchParams | null>(null)
const isSearchMode = computed(() => activeSearchParams.value !== null)

/** SearchFilters 组件实例引用，用于在切换文件时外部调用 clearAll。 */
const searchFiltersRef = ref<InstanceType<typeof SearchFilters> | null>(null)

/* —— 列表（浏览模式：键集分页累加；搜索模式：一次性结果）—— */
const entries = ref<(EntryDto | SearchHitDto)[]>([])
const listLoading = ref(false)
const hasMore = ref(true)
const PAGE = 80

/** 浏览模式：键集分页累加。 */
async function resetAndLoad() {
  entries.value = []
  hasMore.value = true
  await loadMore()
}
async function loadMore() {
  if (listLoading.value || !hasMore.value) return
  listLoading.value = true
  try {
    const after = entries.value.length ? entries.value[entries.value.length - 1].id : undefined
    const batch = await entriesApi.list(props.id, {
      file_id: currentFileId.value ?? undefined,
      task_id: currentTaskId.value ?? undefined,
      after,
      limit: PAGE,
      include_hidden: includeHidden.value,
    })
    entries.value.push(...batch)
    hasMore.value = batch.length === PAGE
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    listLoading.value = false
  }
}

/** 搜索模式：调用混合搜索接口，结果含 relevance。 */
async function runSearch(params: SearchParams) {
  listLoading.value = true
  entries.value = []
  hasMore.value = false
  try {
    const hits = await searchApi.search(props.id, {
      q: params.q,
      file_id: params.file_id,
      state: params.state,
      sort: params.sort,
      include_hidden: params.include_hidden ?? includeHidden.value,
    })
    entries.value = hits
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    listLoading.value = false
  }
}

/** SearchFilters 发出 @search 事件：切换到搜索模式。 */
function onSearch(params: SearchParams) {
  activeSearchParams.value = params
  runSearch(params)
}

/** SearchFilters 发出 @clear 事件：恢复浏览模式。 */
function onSearchClear() {
  activeSearchParams.value = null
  resetAndLoad()
}

watch([currentFileId, includeHidden], () => {
  if (isSearchMode.value && activeSearchParams.value) {
    // 文件/隐藏切换时，SearchFilters 内部 watch 也会触发重新搜索，
    // 此处仅在浏览模式下重置列表。
    return
  }
  resetAndLoad()
})

/** 辅助：判断列表项是否含 relevance 字段（搜索结果）。 */
function isHit(e: EntryDto | SearchHitDto): e is SearchHitDto {
  return 'relevance' in e
}

/** 把 RRF 相关度值（0-1 浮点）转为百分比整数，便于展示。 */
function relevancePct(r: number): number {
  // RRF 分值通常在 0~1 之间；乘以 100 后取整，最高显示 99%
  return Math.min(99, Math.round(r * 100))
}

interface VScrollDetails {
  to: number
}
function onScroll(details: VScrollDetails) {
  if (details.to >= entries.value.length - 12) loadMore()
}

/* —— 选中与编辑 —— */
const selected = ref<EntryDto | null>(null)
const draft = ref('')
const draftState = ref('untranslated')
const saving = ref(false)

/* —— TM 翻译建议 —— */
const suggestions = ref<SuggestionDto[]>([])

/** 应用某条 TM 建议到译文草稿（不自动保存）。 */
function onApplySuggestion(translation: string) {
  draft.value = translation
}

/** 拉取当前词条的 TM 建议；失败时静默降级（非核心功能）。 */
async function fetchSuggestions(entryId: number) {
  try {
    suggestions.value = await suggestionsApi.forEntry(props.id, entryId)
  } catch {
    suggestions.value = []
  }
}

const panelReadOnly = computed(
  () => !isMember.value || (selected.value?.locked === true && !canEditLocked.value),
)

/** 译文或状态相对已保存值有变化。 */
const dirty = computed(
  () =>
    !!selected.value &&
    (draft.value !== selected.value.translation || draftState.value !== selected.value.state),
)

/** 当前选中词条是否有他人正在编辑。 */
const othersEditingSelected = computed(() =>
  selected.value ? otherEditing(selected.value.id) : false,
)

/** 保存按钮形态（标签 / 颜色 / 禁用 / 模式）。 */
const saveBtn = computed(() =>
  computeSaveButton({
    isMember: isMember.value,
    locked: selected.value?.locked === true,
    canEditLocked: canEditLocked.value,
    isManager: isManager.value,
    canReview: canReview.value,
    canEdit: canEdit.value,
    state: selected.value?.state ?? 'untranslated',
    dirty: dirty.value,
    othersEditing: othersEditingSelected.value,
  }),
)

function select(e: EntryDto | SearchHitDto) {
  selected.value = e
  draft.value = e.translation
  draftState.value = e.state
  if (isNarrow.value) mobilePanel.value = true
  sendEditing(e.id)
  void fetchSuggestions(e.id)
}

const sourceLangs = computed(() => project.value?.source_langs ?? [])

async function save() {
  if (!selected.value || saveBtn.value.disabled) return
  const targetState = saveBtn.value.nextState ?? draftState.value
  saving.value = true
  try {
    const updated = await entriesApi.update(props.id, selected.value.id, {
      translation: draft.value,
      state: targetState,
      version: selected.value.version,
    })
    draftState.value = targetState // 推进后同步下拉
    applyUpdated(updated)
    $q.notify({ type: 'positive', message: '已保存', timeout: 900 })
    selectNext()
  } catch (e) {
    const err = e as { response?: { status?: number } }
    if (err.response?.status === 409) {
      $q.notify({ type: 'warning', message: '该词条已被他人修改，已刷新为最新' })
      const fresh = await entriesApi.get(props.id, selected.value.id)
      applyUpdated(fresh)
      select(fresh)
    } else {
      $q.notify({ type: 'negative', message: apiErrorMessage(e, '保存失败') })
    }
  } finally {
    saving.value = false
  }
}

function applyUpdated(u: EntryDto) {
  const idx = entries.value.findIndex((x) => x.id === u.id)
  if (idx >= 0) entries.value[idx] = u
  if (selected.value?.id === u.id) selected.value = u
}

function selectNext() {
  if (!selected.value) return
  const idx = entries.value.findIndex((x) => x.id === selected.value!.id)
  if (idx >= 0 && idx + 1 < entries.value.length) {
    const next = entries.value[idx + 1]
    selected.value = next
    draft.value = next.translation
    draftState.value = next.state
    void fetchSuggestions(next.id)
  }
}

/* —— 实时协作（WebSocket）—— */
function handleRemoteUpdate(entryId: number, _version: number, by: number) {
  if (by === auth.user?.id) return
  if (entries.value.some((e) => e.id === entryId)) {
    entriesApi
      .get(props.id, entryId)
      .then(applyUpdated)
      .catch(() => {})
  }
}
const {
  online: onlineUsers,
  editing: editingMap,
  sendEditing,
} = useRealtime(props.id, { onEntryUpdated: handleRemoteUpdate })
const onlineNames = computed(() =>
  onlineUsers.value
    .filter((uid) => uid !== auth.user?.id)
    .map((uid) => members.value.find((m) => m.user_id === uid)?.username ?? `#${uid}`),
)
function otherEditing(entryId: number): boolean {
  const uid = editingMap.value[entryId]
  return uid !== undefined && uid !== auth.user?.id
}

/** 返回正在编辑指定词条的成员（排除自己）；未找到时返回 null。 */
function editorOf(entryId: number): MemberDto | null {
  const uid = editingMap.value[entryId]
  if (uid === undefined || uid === auth.user?.id) return null
  return members.value.find((m) => m.user_id === uid) ?? null
}

/* —— 戳一下（点击在场头像 → 发即时提示）—— */
const pokeText = ref('')
const pokeSending = ref(false)

/** 跳转到与该成员的私信会话页（编辑器在场头像菜单里的「发私信」）。 */
function openDm(target: MemberDto | null) {
  if (!target) return
  router.push({ name: 'message-thread', params: { userId: target.user_id } })
}

/** 发送戳一下：对准该头像对应的成员，成功后清空输入并 toast 提示。 */
async function sendPoke(target: MemberDto | null) {
  const text = pokeText.value.trim()
  if (!target || !text) return
  pokeSending.value = true
  try {
    await pokeApi.send(props.id, target.user_id, text)
    pokeText.value = ''
    $q.notify({ type: 'positive', message: t('poke.sent'), timeout: 1500 })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    pokeSending.value = false
  }
}

async function toggleFlag(flag: 'locked' | 'hidden') {
  if (!selected.value) return
  try {
    const body =
      flag === 'locked' ? { locked: !selected.value.locked } : { hidden: !selected.value.hidden }
    const updated = await entriesApi.setFlags(props.id, selected.value.id, body)
    applyUpdated(updated)
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '操作失败') })
  }
}

/* —— 历史 —— */
const showHistory = ref(false)
const history = ref<EntryVersionDto[]>([])
async function openHistory() {
  if (!selected.value) return
  try {
    history.value = await entriesApi.history(props.id, selected.value.id)
    showHistory.value = true
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  }
}

onMounted(async () => {
  try {
    const [p, tree, mem] = await Promise.all([
      projectsApi.get(props.id),
      projectsApi.tree(props.id),
      projectsApi.members(props.id),
    ])
    project.value = p.project
    files.value = tree.files
    members.value = mem
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '加载失败') })
  }
  await resetAndLoad()
})
</script>

<template>
  <q-page class="editor-page">
    <!-- toolbar -->
    <div class="editor-bar">
      <q-btn flat dense round icon="mdi-arrow-left" :to="{ name: 'project-info', params: { id: props.id } }">
        <q-tooltip>返回项目</q-tooltip>
      </q-btn>
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
      >
        {{ $t('editor.taskScope', { id: currentTaskId }) }}
      </q-chip>
      <!-- 高级搜索控件：含搜索词时切搜索模式，清空时恢复浏览 -->
      <SearchFilters
        v-if="!isTaskScope"
        ref="searchFiltersRef"
        :file-id="currentFileId"
        :include-hidden="includeHidden"
        @search="onSearch"
        @clear="onSearchClear"
      />
      <q-toggle v-if="isMember && !isTaskScope" v-model="includeHidden" label="含隐藏" dense />
      <!-- 搜索模式指示徽标 -->
      <q-chip v-if="isSearchMode" dense square color="secondary" text-color="dark" icon="mdi-file-search-outline">
        搜索中
      </q-chip>
      <q-chip
        v-if="onlineUsers.length"
        dense
        square
        icon="mdi-account-multiple-outline"
        color="primary"
        text-color="dark"
        class="prts-mono"
      >
        {{ onlineUsers.length }}
        <q-tooltip v-if="onlineNames.length">协作中：{{ onlineNames.join('、') }}</q-tooltip>
      </q-chip>
    </div>

    <div class="editor-body">
      <!-- list pane -->
      <div v-show="!isNarrow || !mobilePanel" class="ed-pane ed-pane--list">
        <q-virtual-scroll :items="entries" class="entry-list" @virtual-scroll="onScroll">
          <template #default="{ item }">
            <div class="entry-row" :class="{ active: item.id === selected?.id }" @click="select(item)">
              <span class="state-dot" :class="'state-' + item.state" />
              <div class="entry-row__body">
                <div class="entry-row__key prts-mono">{{ item.key }}</div>
                <div class="entry-row__preview">
                  {{ item.translation || Object.values(item.original)[0] || '—' }}
                </div>
              </div>
              <!-- 搜索模式：显示相关度百分比 -->
              <span v-if="isHit(item)" class="relevance-badge" :title="'相关度 ' + relevancePct(item.relevance) + '%'">
                {{ relevancePct(item.relevance) }}%
              </span>
              <q-icon v-if="item.locked" name="mdi-lock-outline" size="14px" class="prts-dim" />
              <q-icon v-if="item.hidden" name="mdi-eye-off-outline" size="14px" class="prts-dim" />
              <template v-if="otherEditing(item.id)">
                <q-avatar
                  v-if="editorOf(item.id)?.avatar_url"
                  size="18px"
                  class="poke-avatar"
                  @click.stop
                >
                  <img :src="editorOf(item.id)!.avatar_url!" :alt="editorOf(item.id)!.username" />
                  <q-tooltip>{{ editorOf(item.id)!.username }} · {{ editorOf(item.id)!.role }} · {{ t('editor.editingNow') }}</q-tooltip>
                  <q-menu anchor="top right" self="bottom right">
                    <div class="poke-compose" @click.stop>
                      <div class="prts-label q-mb-xs">{{ t('poke.composeTitle', { name: editorOf(item.id)!.username }) }}</div>
                      <q-input
                        v-model="pokeText"
                        dense
                        outlined
                        autofocus
                        counter
                        maxlength="140"
                        :placeholder="t('poke.placeholder')"
                        @keyup.enter="sendPoke(editorOf(item.id))"
                      />
                      <div class="row justify-end q-mt-sm q-gutter-xs">
                        <q-btn
                          v-close-popup
                          flat
                          no-caps
                          dense
                          icon="mdi-email-outline"
                          :label="t('dm.entry')"
                          @click="openDm(editorOf(item.id))"
                        />
                        <q-btn
                          v-close-popup
                          unelevated
                          no-caps
                          dense
                          color="primary"
                          text-color="dark"
                          :label="t('poke.send')"
                          :loading="pokeSending"
                          :disable="!pokeText.trim()"
                          @click="sendPoke(editorOf(item.id))"
                        />
                      </div>
                    </div>
                  </q-menu>
                </q-avatar>
                <q-avatar
                  v-else-if="editorOf(item.id)"
                  size="18px"
                  color="amber"
                  text-color="dark"
                  class="poke-avatar"
                  @click.stop
                >
                  {{ editorOf(item.id)!.username.charAt(0).toUpperCase() }}
                  <q-tooltip>{{ editorOf(item.id)!.username }} · {{ editorOf(item.id)!.role }} · {{ t('editor.editingNow') }}</q-tooltip>
                  <q-menu anchor="top right" self="bottom right">
                    <div class="poke-compose" @click.stop>
                      <div class="prts-label q-mb-xs">{{ t('poke.composeTitle', { name: editorOf(item.id)!.username }) }}</div>
                      <q-input
                        v-model="pokeText"
                        dense
                        outlined
                        autofocus
                        counter
                        maxlength="140"
                        :placeholder="t('poke.placeholder')"
                        @keyup.enter="sendPoke(editorOf(item.id))"
                      />
                      <div class="row justify-end q-mt-sm q-gutter-xs">
                        <q-btn
                          v-close-popup
                          flat
                          no-caps
                          dense
                          icon="mdi-email-outline"
                          :label="t('dm.entry')"
                          @click="openDm(editorOf(item.id))"
                        />
                        <q-btn
                          v-close-popup
                          unelevated
                          no-caps
                          dense
                          color="primary"
                          text-color="dark"
                          :label="t('poke.send')"
                          :loading="pokeSending"
                          :disable="!pokeText.trim()"
                          @click="sendPoke(editorOf(item.id))"
                        />
                      </div>
                    </div>
                  </q-menu>
                </q-avatar>
                <q-icon v-else name="mdi-pencil-outline" size="13px" color="amber">
                  <q-tooltip>{{ t('editor.editingNow') }}</q-tooltip>
                </q-icon>
              </template>
            </div>
          </template>
        </q-virtual-scroll>
        <div v-if="listLoading" class="row justify-center q-pa-sm">
          <q-spinner color="primary" size="20px" />
        </div>
        <div v-else-if="entries.length === 0" class="prts-empty">
          {{ isSearchMode ? '未找到相关词条' : '无匹配词条' }}
        </div>
      </div>

      <!-- panel pane -->
      <div v-show="!isNarrow || mobilePanel" class="ed-pane ed-pane--panel">
        <q-btn
          v-if="isNarrow"
          flat
          dense
          no-caps
          icon="mdi-arrow-left"
          label="词条列表"
          class="q-ma-sm"
          @click="mobilePanel = false"
        />
        <div v-if="!selected" class="prts-empty" style="padding-top: 100px">
          从{{ isNarrow ? '列表' : '左侧' }}选择一个词条开始翻译
        </div>
        <div v-else class="panel">
          <div class="row items-center q-mb-sm">
            <div class="prts-label">KEY</div>
            <div class="prts-mono q-ml-sm ellipsis" style="font-size: 13px; max-width: 50%">
              {{ selected.key }}
            </div>
            <q-space />
            <q-btn flat dense round size="sm" icon="mdi-history" @click="openHistory">
              <q-tooltip>历史</q-tooltip>
            </q-btn>
            <q-btn
              v-if="canFlag"
              flat
              dense
              round
              size="sm"
              :icon="selected.locked ? 'lock' : 'lock_open'"
              :color="selected.locked ? 'amber' : undefined"
              @click="toggleFlag('locked')"
            >
              <q-tooltip>{{ selected.locked ? '解锁' : '锁定' }}</q-tooltip>
            </q-btn>
            <q-btn
              v-if="canFlag"
              flat
              dense
              round
              size="sm"
              :icon="selected.hidden ? 'visibility_off' : 'visibility'"
              @click="toggleFlag('hidden')"
            >
              <q-tooltip>{{ selected.hidden ? '取消隐藏' : '隐藏' }}</q-tooltip>
            </q-btn>
          </div>

          <div class="orig-block">
            <div v-for="lang in sourceLangs" :key="lang" class="orig-row">
              <div class="prts-label orig-lang">{{ lang }}</div>
              <div class="orig-text">{{ selected.original[lang] ?? '—' }}</div>
            </div>
            <div v-if="selected.context" class="orig-row">
              <div class="prts-label orig-lang">注释</div>
              <div class="orig-text prts-dim">{{ selected.context }}</div>
            </div>
          </div>

          <div class="prts-label q-mt-md q-mb-xs">译文 → {{ project?.target_lang }}</div>
          <q-input
            v-model="draft"
            type="textarea"
            outlined
            autogrow
            :readonly="panelReadOnly"
            input-class="prts-translation"
            :input-style="{ minHeight: '120px' }"
          />

          <!-- TM 翻译建议面板（无建议时不渲染）-->
          <SuggestionsPanel :suggestions="suggestions" @apply="onApplySuggestion" />

          <div class="row items-center q-mt-md q-gutter-sm">
            <q-select
              v-model="draftState"
              :options="availableStates"
              :option-label="(s) => STATE_LABELS[s] ?? s"
              dense
              outlined
              emit-value
              map-options
              :disable="panelReadOnly"
              style="min-width: 130px"
            />
            <q-space />
            <span v-if="panelReadOnly" class="prts-dim prts-mono" style="font-size: 12px">
              {{ selected.locked ? '已锁定 · 只读' : '无编辑权限' }}
            </span>
            <q-btn
              unelevated
              no-caps
              :color="saveBtn.color"
              :text-color="saveBtn.color ? 'dark' : undefined"
              icon="mdi-content-save-outline"
              :label="t('editor.btn_' + saveBtn.labelKey)"
              :loading="saving"
              :disable="saveBtn.disabled"
              @click="save"
            >
              <q-tooltip v-if="saveBtn.mode === 'force'">{{ t('editor.forceHint') }}</q-tooltip>
              <q-tooltip v-else-if="saveBtn.disabled && saveBtn.mode === 'none' && othersEditingSelected">
                {{ t('editor.othersEditingHint') }}
              </q-tooltip>
            </q-btn>
          </div>
        </div>
      </div>
    </div>

    <!-- history dialog -->
    <q-dialog v-model="showHistory">
      <q-card style="width: 560px; max-width: 94vw">
        <q-card-section><div class="prts-h2">词条历史</div></q-card-section>
        <q-card-section style="max-height: 60vh; overflow: auto">
          <q-timeline v-if="history.length" color="primary">
            <q-timeline-entry
              v-for="(h, i) in history"
              :key="i"
              :subtitle="new Date(h.created_at).toLocaleString()"
            >
              <template #title>
                <span class="prts-mono" style="font-size: 13px">v{{ h.version }} · {{ h.kind }}</span>
                <q-badge v-if="h.state" outline class="q-ml-sm" :label="stateLabel(h.state)" />
              </template>
              <div v-if="h.translation" class="prts-translation">{{ h.translation }}</div>
            </q-timeline-entry>
          </q-timeline>
          <div v-else class="prts-empty">暂无历史</div>
        </q-card-section>
        <q-card-actions align="right"><q-btn v-close-popup flat no-caps label="关闭" /></q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>

<style scoped>
.editor-page {
  height: calc(100vh - var(--prts-nav-h));
  display: flex;
  flex-direction: column;
}
.editor-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--prts-border);
  background: var(--prts-panel);
  flex-wrap: wrap;
}
.editor-title {
  font-size: 14px;
  max-width: 200px;
}
.editor-fileselect {
  min-width: 170px;
  max-width: 260px;
}
.editor-body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.ed-pane {
  height: 100%;
  min-height: 0;
}
.ed-pane--list {
  width: 340px;
  border-right: 1px solid var(--prts-border);
  display: flex;
  flex-direction: column;
}
.ed-pane--panel {
  flex: 1;
  overflow: auto;
}
.entry-list {
  flex: 1;
  min-height: 0;
}
.entry-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--prts-border-soft);
  cursor: pointer;
}
.entry-row:hover {
  background: var(--prts-panel-2);
}
.entry-row.active {
  background: var(--prts-accent-dim);
  box-shadow: inset 2px 0 0 var(--prts-accent);
}
.entry-row__body {
  min-width: 0;
  flex: 1;
}
.entry-row__key {
  font-size: 11px;
  color: var(--prts-text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.entry-row__preview {
  font-size: 13px;
  color: var(--prts-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.panel {
  padding: 18px 22px 40px;
}
.orig-block {
  border: 1px solid var(--prts-border);
  border-radius: var(--prts-radius);
  background: var(--prts-bg-elev);
  overflow: hidden;
}
.orig-row {
  display: flex;
  gap: 14px;
  padding: 11px 14px;
  border-bottom: 1px solid var(--prts-border-soft);
}
.orig-row:last-child {
  border-bottom: none;
}
.orig-lang {
  flex: 0 0 52px;
  padding-top: 2px;
}
.orig-text {
  font-size: 14px;
  line-height: 1.6;
  white-space: pre-wrap;
  color: var(--prts-text-strong);
}
:deep(.prts-translation) {
  font-size: 14px;
  line-height: 1.7;
}

/* 在场头像：可点击发「戳一下」 */
.poke-avatar {
  cursor: pointer;
}
.poke-compose {
  width: 240px;
  padding: 10px 12px;
}

/* 搜索结果相关度徽标 */
.relevance-badge {
  flex-shrink: 0;
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  color: var(--prts-text-dim);
  background: var(--prts-bg-elev);
  border: 1px solid var(--prts-border);
  border-radius: 3px;
  padding: 1px 4px;
  line-height: 1.4;
}

/* 移动端：单栏 + 列表/面板切换 */
@media (max-width: 1023px) {
  .ed-pane--list {
    width: 100%;
    border-right: none;
  }
}
</style>

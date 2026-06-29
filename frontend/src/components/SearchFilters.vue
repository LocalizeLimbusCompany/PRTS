<script setup lang="ts">
/**
 * SearchFilters.vue — 编辑器左侧列表的高级搜索控件。
 *
 * 当 q 非空时，向父组件 emit "search" 事件，携带搜索参数对象；
 * 当 q 清空时，emit "clear" 事件，父组件恢复普通键集浏览。
 */
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { STATE_LABELS, STATE_ORDER } from '@/lib/states'

export interface SearchParams {
  q: string
  state?: string
  sort?: string
  file_id?: number
  include_hidden?: boolean
}

const props = defineProps<{
  /** 当前选中的文件 id（从父组件传入，供搜索时一并传递）。 */
  fileId?: number | null
  /** 是否含隐藏（从父组件传入并双向同步）。 */
  includeHidden?: boolean
}>()

const emit = defineEmits<{
  /** 用户发起搜索（q 非空）。 */
  (e: 'search', params: SearchParams): void
  /** 搜索被清空，父组件应恢复浏览模式。 */
  (e: 'clear'): void
}>()

const { t } = useI18n()

const q = ref('')
const stateFilter = ref<string[]>([])
const sort = ref<string>('relevance')

const sortOptions = [
  { label: () => t('editor.sortRelevance'), value: 'relevance' },
  { label: () => t('editor.sortKey'), value: 'key' },
  { label: () => t('editor.sortUpdatedAt'), value: 'updated_at' },
]

// 状态选项：key + 中文标签
const stateOptions = STATE_ORDER.map((s) => ({ label: STATE_LABELS[s] ?? s, value: s }))

let debounceTimer: ReturnType<typeof setTimeout> | undefined

function emitSearch() {
  if (!q.value.trim()) {
    emit('clear')
    return
  }
  const params: SearchParams = {
    q: q.value.trim(),
    sort: sort.value,
  }
  if (stateFilter.value.length) params.state = stateFilter.value.join(',')
  if (props.fileId != null) params.file_id = props.fileId
  if (props.includeHidden) params.include_hidden = true
  emit('search', params)
}

watch(q, () => {
  clearTimeout(debounceTimer)
  if (!q.value.trim()) {
    // 立即通知清空
    emit('clear')
    return
  }
  debounceTimer = setTimeout(emitSearch, 300)
})

watch([stateFilter, sort], () => {
  if (q.value.trim()) {
    clearTimeout(debounceTimer)
    debounceTimer = setTimeout(emitSearch, 150)
  }
})

// 当文件/隐藏变化时（父组件变更），若当前有搜索词则重新搜索
watch(
  () => [props.fileId, props.includeHidden],
  () => {
    if (q.value.trim()) {
      clearTimeout(debounceTimer)
      debounceTimer = setTimeout(emitSearch, 150)
    }
  },
)

function clearAll() {
  q.value = ''
  stateFilter.value = []
  sort.value = 'relevance'
  emit('clear')
}

/** 供父组件外部清空（例如切换文件时）。 */
defineExpose({ clearAll })
</script>

<template>
  <div class="sf-root">
    <!-- 搜索文本框 -->
    <q-input
      v-model="q"
      dense
      outlined
      clearable
      :placeholder="t('editor.searchPlaceholder')"
      class="sf-input"
      @clear="clearAll"
    >
      <template #prepend>
        <q-icon name="search" />
      </template>
    </q-input>

    <!-- 当搜索词非空时，展示搜索附加控件 -->
    <template v-if="q.trim()">
      <!-- 状态多选 -->
      <q-select
        v-model="stateFilter"
        :options="stateOptions"
        option-label="label"
        option-value="value"
        emit-value
        map-options
        dense
        outlined
        multiple
        options-dense
        :placeholder="t('editor.stateFilter')"
        class="sf-state"
      />
      <!-- 排序下拉 -->
      <q-select
        v-model="sort"
        :options="sortOptions.map((o) => ({ label: o.label(), value: o.value }))"
        option-label="label"
        option-value="value"
        emit-value
        map-options
        dense
        outlined
        options-dense
        :label="t('editor.sortLabel')"
        class="sf-sort"
      />
    </template>
  </div>
</template>

<style scoped>
.sf-root {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  flex: 1;
  min-width: 0;
}
.sf-input {
  min-width: 180px;
  flex: 1;
}
.sf-state {
  min-width: 130px;
}
.sf-sort {
  min-width: 130px;
}
</style>

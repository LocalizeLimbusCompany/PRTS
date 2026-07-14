<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { FileDto, StructuredSearchRequest } from '@/api'
import AdvancedFilterDialog from '@/components/editor/AdvancedFilterDialog.vue'
import SearchBar from '@/components/editor/SearchBar.vue'

defineProps<{
  files: FileDto[]
  sourceLangs: string[]
  currentFileId: number | null
  currentTaskId: number | null
  canIncludeHidden: boolean
}>()
const emit = defineEmits<{
  (event: 'search', request: StructuredSearchRequest): void
  (event: 'clear'): void
}>()
const { t } = useI18n()
const advancedOpen = ref(false)
</script>

<template>
  <div class="search-filters">
    <SearchBar
      class="search-filters__bar"
      :current-file-id="currentFileId"
      @search="emit('search', $event)"
      @clear="emit('clear')"
    />
    <q-btn
      flat
      round
      dense
      icon="mdi-tune-variant"
      :aria-label="t('editor.advancedFilters')"
      @click="advancedOpen = true"
    >
      <q-tooltip>{{ t('editor.advancedFilters') }}</q-tooltip>
    </q-btn>
    <AdvancedFilterDialog
      v-model="advancedOpen"
      :files="files"
      :source-langs="sourceLangs"
      :current-file-id="currentFileId"
      :current-task-id="currentTaskId"
      :can-include-hidden="canIncludeHidden"
      @search="emit('search', $event)"
    />
  </div>
</template>

<style scoped>
.search-filters {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px;
  border-bottom: 1px solid var(--prts-border);
}
.search-filters__bar {
  flex: 1;
  min-width: 0;
}
</style>

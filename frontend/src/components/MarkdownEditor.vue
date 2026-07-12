<script setup lang="ts">
import { computed, ref } from 'vue'

import MarkdownView from './MarkdownView.vue'

const model = defineModel<string>({ default: '' })
const props = withDefaults(
  defineProps<{
    label?: string
    placeholder?: string
    readonly?: boolean
    maxLength?: number
  }>(),
  { label: '', placeholder: '', readonly: false, maxLength: 20_000 },
)

const preview = ref(false)
const remaining = computed(() => props.maxLength - model.value.length)
</script>

<template>
  <section class="markdown-editor">
    <header class="markdown-editor__head">
      <span class="prts-label">{{ props.label }}</span>
      <q-btn-toggle
        v-model="preview"
        dense
        flat
        no-caps
        :options="[
          { label: $t('markdown.write'), value: false },
          { label: $t('markdown.preview'), value: true },
        ]"
      />
    </header>
    <MarkdownView v-if="preview" class="markdown-editor__preview" :source="model" />
    <q-input
      v-else
      v-model="model"
      type="textarea"
      outlined
      autogrow
      :readonly="props.readonly"
      :maxlength="props.maxLength"
      :placeholder="props.placeholder"
    />
    <footer class="markdown-editor__foot">
      <span>{{ $t('markdown.supported') }}</span>
      <span>{{ $t('markdown.remaining', { count: remaining }) }}</span>
    </footer>
  </section>
</template>

<style scoped>
.markdown-editor {
  display: grid;
  gap: 8px;
}

.markdown-editor__head,
.markdown-editor__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.markdown-editor__preview {
  min-height: 116px;
  padding: 12px;
  border: 1px solid var(--prts-border);
  border-radius: 2px;
}

.markdown-editor__foot {
  color: var(--prts-text-dim);
  font-size: 12px;
}
</style>

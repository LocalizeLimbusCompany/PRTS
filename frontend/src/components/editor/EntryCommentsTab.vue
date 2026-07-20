<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, entryCommentsApi, type EntryCommentDto } from '@/api'
import MarkdownEditor from '@/components/MarkdownEditor.vue'
import MarkdownView from '@/components/MarkdownView.vue'

const props = defineProps<{ projectId: number; entryId: number; refreshToken?: number }>()
const $q = useQuasar()
const { t } = useI18n()
const comments = ref<EntryCommentDto[]>([])
const canComment = ref(false)
const nextAfter = ref<number | null>(null)
const content = ref('')
const editingId = ref<number | null>(null)
const editingContent = ref('')
const loading = ref(false)
const saving = ref(false)
let loadGeneration = 0

async function load(append = false) {
  const generation = ++loadGeneration
  loading.value = true
  try {
    const page = await entryCommentsApi.list(props.projectId, props.entryId, {
      after: append ? (nextAfter.value ?? undefined) : undefined,
      limit: 100,
    })
    if (generation !== loadGeneration) return
    comments.value = append ? [...comments.value, ...page.items] : page.items
    nextAfter.value = page.next_after
    canComment.value = page.can_comment
  } catch (error) {
    if (generation === loadGeneration) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    }
  } finally {
    if (generation === loadGeneration) loading.value = false
  }
}

async function submit() {
  if (!content.value.trim()) return
  saving.value = true
  try {
    await entryCommentsApi.create(props.projectId, props.entryId, content.value)
    content.value = ''
    await load(false)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    saving.value = false
  }
}

function startEditing(comment: EntryCommentDto) {
  editingId.value = comment.id
  editingContent.value = comment.content
}

function cancelEditing() {
  editingId.value = null
  editingContent.value = ''
}

async function saveEditing(comment: EntryCommentDto) {
  if (!editingContent.value.trim()) return
  saving.value = true
  try {
    await entryCommentsApi.update(props.projectId, props.entryId, comment.id, editingContent.value)
    cancelEditing()
    await load(false)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    saving.value = false
  }
}

function remove(comment: EntryCommentDto) {
  $q.dialog({
    title: t('editor.deleteComment'),
    message: t('editor.deleteCommentConfirm'),
    cancel: true,
  }).onOk(async () => {
    try {
      await entryCommentsApi.remove(props.projectId, props.entryId, comment.id)
      if (editingId.value === comment.id) cancelEditing()
      await load(false)
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    }
  })
}

function refresh() {
  cancelEditing()
  comments.value = []
  canComment.value = false
  nextAfter.value = null
  void load(false)
}

onMounted(refresh)
watch(() => [props.entryId, props.refreshToken], refresh)
</script>

<template>
  <div class="comments-tab">
    <div class="comments-tab__list">
      <article v-for="comment in comments" :key="comment.id" class="comment-card">
        <header>
          <q-avatar size="26px" color="primary" text-color="dark"
            ><img v-if="comment.author_avatar_url" :src="comment.author_avatar_url" alt="" /><span
              v-else
              >{{ comment.author_name.charAt(0).toUpperCase() }}</span
            ></q-avatar
          ><strong>{{ comment.author_name }}</strong
          ><span class="prts-dim">{{ new Date(comment.created_at).toLocaleString() }}</span
          ><q-space /><q-btn
            v-if="comment.can_edit && editingId !== comment.id"
            flat
            round
            dense
            icon="mdi-pencil-outline"
            :aria-label="$t('editor.editComment')"
            @click="startEditing(comment)"
          /><q-btn
            v-if="comment.can_delete"
            flat
            round
            dense
            icon="mdi-delete-outline"
            :aria-label="$t('editor.deleteComment')"
            @click="remove(comment)"
          />
        </header>
        <div v-if="comment.deleted" class="prts-dim">{{ $t('editor.commentDeleted') }}</div>
        <div v-else-if="editingId === comment.id" class="comment-card__edit">
          <MarkdownEditor
            v-model="editingContent"
            :label="$t('editor.editComment')"
            :max-length="4000"
          />
          <div class="row justify-end q-gutter-xs">
            <q-btn flat no-caps :label="$t('common.cancel')" @click="cancelEditing" />
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              icon="mdi-content-save-outline"
              :label="$t('editor.saveComment')"
              :loading="saving"
              :disable="!editingContent.trim()"
              @click="saveEditing(comment)"
            />
          </div>
        </div>
        <MarkdownView v-else :source="comment.content" />
      </article>
      <div v-if="comments.length === 0" class="prts-empty">{{ $t('editor.noComments') }}</div>
      <q-btn
        v-if="nextAfter !== null"
        flat
        no-caps
        icon="mdi-chevron-down"
        :label="$t('editor.loadMoreComments')"
        :loading="loading"
        @click="load(true)"
      />
    </div>
    <div v-if="canComment" class="comments-tab__compose">
      <MarkdownEditor v-model="content" :label="$t('editor.comment')" :max-length="4000" />
      <q-btn
        unelevated
        no-caps
        color="primary"
        text-color="dark"
        icon="mdi-send-outline"
        :label="$t('editor.publishComment')"
        :loading="saving"
        :disable="!content.trim()"
        @click="submit"
      />
    </div>
    <div v-else class="prts-dim comments-tab__compose">{{ $t('editor.commentReadOnly') }}</div>
  </div>
</template>

<style scoped>
.comments-tab {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}
.comments-tab__list {
  display: grid;
  gap: 10px;
  padding: 10px;
}
.comment-card {
  padding: 10px;
  border: 1px solid var(--prts-border-soft);
  background: var(--prts-panel-2);
}
.comment-card header {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-bottom: 8px;
  font-size: 11px;
}
.comment-card__edit {
  display: grid;
  gap: 8px;
}
.comments-tab__compose {
  display: grid;
  gap: 8px;
  margin-top: auto;
  padding: 10px;
  border-top: 1px solid var(--prts-border);
}
</style>

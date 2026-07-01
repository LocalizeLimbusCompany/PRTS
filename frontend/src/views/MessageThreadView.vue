<script setup lang="ts">
/**
 * MessageThreadView.vue —— 会话线程：与某用户的私信往来。
 *
 * 挂载/切换会话时：拉对方公开资料 + 会话消息（键集，倒序→正序渲染）+ 标记已读 +
 * 刷新全局未读；订阅 shared stream 即时追加对方新消息；底部发送框（≤2000 字，
 * Enter 发送、Shift+Enter 换行、IME 组合中不误发）。
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, messagesApi, usersApi, type MessageDto, type UserDto } from '@/api'
import { useAuthStore } from '@/stores/auth'
import { useMessages } from '@/composables/useMessages'
import { useUserStream, type UserStreamEvent } from '@/composables/useUserStream'

const props = defineProps<{ userId: number }>()
const $q = useQuasar()
const { t } = useI18n()
const auth = useAuthStore()

const meId = computed(() => auth.user?.id ?? null)
const other = ref<UserDto | null>(null)
const messages = ref<MessageDto[]>([]) // 按时间正序（旧→新，最新在底部）
const draft = ref('')
const sending = ref(false)
const loadingMore = ref(false)
const hasMore = ref(true)
const scrollEl = ref<HTMLElement | null>(null)

const PAGE = 50

let unsub: (() => void) | undefined

/** 滚动到底部（新消息/进入时）。 */
function scrollToBottom() {
  void nextTick(() => {
    const el = scrollEl.value
    if (el) el.scrollTop = el.scrollHeight
  })
}

/** 标记与某用户会话已读并刷新全局未读/会话列表。 */
async function markReadAndRefresh(uid: number) {
  try {
    await messagesApi.markRead(uid)
    await useMessages().refresh()
  } catch {
    /* 静默降级 */
  }
}

/** 加载更早的消息（键集：以当前最旧一条的 id 为游标向前翻）。 */
async function loadEarlier() {
  if (loadingMore.value || !hasMore.value || !messages.value.length) return
  loadingMore.value = true
  try {
    const oldestId = messages.value[0].id
    const older = await messagesApi.conversation(props.userId, oldestId, PAGE)
    messages.value = [...older.slice().reverse(), ...messages.value]
    hasMore.value = older.length === PAGE
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    loadingMore.value = false
  }
}

/** 发送一条私信：成功后本地追加、清空、刷新会话列表。 */
async function send() {
  const content = draft.value.trim()
  if (!content || sending.value) return
  sending.value = true
  try {
    const { id } = await messagesApi.send(props.userId, content)
    messages.value.push({
      id,
      sender_id: meId.value ?? 0,
      recipient_id: props.userId,
      content,
      read_at: null,
      created_at: new Date().toISOString(),
    })
    draft.value = ''
    scrollToBottom()
    void useMessages().refresh() // 更新会话列表最后一条/排序
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    sending.value = false
  }
}

/** Enter 发送；IME 组合中（如中文输入）不触发，Shift+Enter 换行（.exact）。 */
function onEnterKey(e: KeyboardEvent) {
  if (e.isComposing) return
  e.preventDefault()
  void send()
}

/** shared stream：对方（本会话）发来的新私信即时追加并标记已读。 */
function onStreamEvent(msg: UserStreamEvent) {
  if (msg.type !== 'DmMessage' || msg.from_user_id !== props.userId || typeof msg.id !== 'number') {
    return
  }
  messages.value.push({
    id: msg.id,
    sender_id: props.userId,
    recipient_id: meId.value ?? 0,
    content: typeof msg.content === 'string' ? msg.content : '',
    read_at: null,
    created_at: msg.created_at ?? new Date().toISOString(),
  })
  void markReadAndRefresh(props.userId)
  scrollToBottom()
}

onMounted(() => {
  unsub = useUserStream().onEvent(onStreamEvent)
})

onUnmounted(() => {
  useMessages().setActiveThread(null)
  unsub?.()
})

// 初次进入与「会话间切换（同组件仅换 param）」都经此加载。
watch(
  () => props.userId,
  async (uid) => {
    useMessages().setActiveThread(uid)
    other.value = null
    messages.value = []
    hasMore.value = true
    try {
      const [u, convo] = await Promise.all([
        usersApi.getUser(uid),
        messagesApi.conversation(uid, undefined, PAGE),
      ])
      other.value = u
      messages.value = convo.slice().reverse() // API 返回 id 降序 → 正序渲染
      hasMore.value = convo.length === PAGE
    } catch (e) {
      $q.notify({ type: 'negative', message: apiErrorMessage(e) })
    }
    await markReadAndRefresh(uid)
    scrollToBottom()
  },
  { immediate: true },
)

/** 消息时间（本地化短格式）。 */
function shortTime(iso: string): string {
  return new Date(iso).toLocaleString()
}
</script>

<template>
  <q-page class="mt-page">
    <!-- 头部：返回 + 对方头名 -->
    <div class="mt-header">
      <q-btn flat dense round icon="arrow_back" :to="{ name: 'messages' }">
        <q-tooltip>{{ t('messages.title') }}</q-tooltip>
      </q-btn>
      <q-avatar size="32px" color="primary" text-color="dark">
        <img v-if="other?.avatar_url" :src="other.avatar_url" :alt="other?.username" />
        <span v-else>{{ (other?.username ?? '').slice(0, 2).toUpperCase() }}</span>
      </q-avatar>
      <div class="mt-title ellipsis">{{ other?.username ?? '…' }}</div>
    </div>

    <!-- 消息区 -->
    <div ref="scrollEl" class="mt-body">
      <div v-if="hasMore && messages.length" class="row justify-center q-py-sm">
        <q-btn
          flat
          dense
          no-caps
          size="sm"
          :loading="loadingMore"
          :label="t('messages.loadEarlier')"
          @click="loadEarlier"
        />
      </div>

      <div
        v-for="m in messages"
        :key="m.id"
        class="mt-row"
        :class="{ 'mt-row--mine': m.sender_id === meId }"
      >
        <div class="mt-bubble">
          <div class="mt-content">{{ m.content }}</div>
          <div class="mt-time">{{ shortTime(m.created_at) }}</div>
        </div>
      </div>

      <div v-if="!messages.length" class="prts-empty mt-empty">{{ t('messages.threadEmpty') }}</div>
    </div>

    <!-- 发送区 -->
    <div class="mt-compose">
      <q-input
        v-model="draft"
        type="textarea"
        outlined
        dense
        autogrow
        maxlength="2000"
        :placeholder="t('messages.placeholder')"
        class="mt-input"
        @keydown.enter.exact="onEnterKey"
      />
      <q-btn
        unelevated
        no-caps
        color="primary"
        text-color="dark"
        icon="send"
        :label="t('messages.send')"
        :loading="sending"
        :disable="!draft.trim()"
        @click="send"
      />
    </div>
  </q-page>
</template>

<style scoped>
.mt-page {
  height: calc(100vh - var(--prts-nav-h));
  display: flex;
  flex-direction: column;
  max-width: 720px;
  margin: 0 auto;
  width: 100%;
}
.mt-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--prts-border);
  background: var(--prts-panel);
}
.mt-title {
  font-weight: 600;
  font-size: 15px;
}
.mt-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 16px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.mt-row {
  display: flex;
  justify-content: flex-start;
}
.mt-row--mine {
  justify-content: flex-end;
}
.mt-bubble {
  max-width: 72%;
  padding: 8px 12px;
  border-radius: 12px;
  background: var(--prts-panel-2);
  border: 1px solid var(--prts-border-soft);
}
.mt-row--mine .mt-bubble {
  background: var(--prts-accent-dim);
  border-color: var(--prts-accent);
}
.mt-content {
  font-size: 14px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--prts-text-strong);
}
.mt-time {
  margin-top: 4px;
  font-size: 10px;
  color: var(--prts-text-dim);
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.mt-empty {
  margin: auto;
}
.mt-compose {
  display: flex;
  align-items: flex-end;
  gap: 8px;
  padding: 10px 14px;
  border-top: 1px solid var(--prts-border);
  background: var(--prts-panel);
}
.mt-input {
  flex: 1;
}
</style>

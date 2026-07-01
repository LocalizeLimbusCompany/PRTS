<script setup lang="ts">
/**
 * MessagesView.vue —— 私信会话列表：每个对话方一行（头像 + 用户名 + 最后一条 +
 * 相对时间 + 未读徽标），点击进入会话线程。空态提示去编辑器/成员处发起。
 */
import { onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import { useMessages } from '@/composables/useMessages'
import { useAuthStore } from '@/stores/auth'
import type { ThreadDto } from '@/api'

const router = useRouter()
const auth = useAuthStore()
const { t } = useI18n()
const { threads, refresh } = useMessages()

onMounted(refresh)

function open(th: ThreadDto) {
  router.push({ name: 'message-thread', params: { userId: th.other_user_id } })
}

/** 相对时间（粗粒度，复用 notifications 的文案口径）。 */
function relativeTime(iso: string): string {
  const diffMs = Date.now() - new Date(iso).getTime()
  const min = Math.floor(diffMs / 60_000)
  if (min < 1) return t('notifications.justNow')
  if (min < 60) return t('notifications.minutesAgo', { n: min })
  const hr = Math.floor(min / 60)
  if (hr < 24) return t('notifications.hoursAgo', { n: hr })
  const day = Math.floor(hr / 24)
  return t('notifications.daysAgo', { n: day })
}

/** 会话最后一条预览：我方发出的加「我: 」前缀。 */
function preview(th: ThreadDto): string {
  const mine = th.last_sender_id === auth.user?.id
  return mine ? `${t('messages.you')}: ${th.last_content}` : th.last_content
}
</script>

<template>
  <q-page class="msg-page">
    <div class="msg-wrap">
      <div class="prts-h2 q-mb-md">{{ t('messages.title') }}</div>

      <q-list v-if="threads.length" separator class="msg-list">
        <q-item v-for="th in threads" :key="th.other_user_id" clickable @click="open(th)">
          <q-item-section avatar>
            <q-avatar size="42px" color="primary" text-color="dark">
              <img v-if="th.avatar_url" :src="th.avatar_url" :alt="th.username" />
              <span v-else>{{ th.username.slice(0, 2).toUpperCase() }}</span>
            </q-avatar>
          </q-item-section>
          <q-item-section>
            <q-item-label class="msg-name">{{ th.username }}</q-item-label>
            <q-item-label caption lines="1">{{ preview(th) }}</q-item-label>
          </q-item-section>
          <q-item-section side top>
            <q-item-label caption>{{ relativeTime(th.last_created_at) }}</q-item-label>
            <q-badge v-if="th.unread > 0" color="negative" rounded class="q-mt-xs">
              {{ th.unread }}
            </q-badge>
          </q-item-section>
        </q-item>
      </q-list>

      <div v-else class="prts-empty msg-empty">{{ t('messages.empty') }}</div>
    </div>
  </q-page>
</template>

<style scoped>
.msg-page {
  padding: 24px 16px;
}
.msg-wrap {
  max-width: 720px;
  margin: 0 auto;
}
.msg-list {
  border: 1px solid var(--prts-border);
  border-radius: var(--prts-radius);
  overflow: hidden;
  background: var(--prts-panel);
}
.msg-name {
  font-weight: 600;
}
.msg-empty {
  padding: 60px 0;
  text-align: center;
}
</style>

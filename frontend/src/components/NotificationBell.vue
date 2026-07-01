<script setup lang="ts">
/**
 * NotificationBell.vue — 顶栏铃铛：未读徽标 + 最近通知列表 + 全部已读。
 *
 * 状态来自 useNotifications() 模块级单例（由 App.vue 在登录后启动/登出时断开），
 * 本组件只负责展示与交互，不重复发起连接。
 */
import { useI18n } from 'vue-i18n'

import { useNotifications } from '@/composables/useNotifications'
import type { NotificationDto } from '@/api'

const { t } = useI18n()
const { unread, items, markAllRead } = useNotifications()

/** 粗粒度相对时间（够用即可，避免引入新的日期库）。 */
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

/** 通知展示文案：目前仅 poke 类型有专属格式，其余类型兜底展示 type。 */
function summary(n: NotificationDto): string {
  if (n.type === 'poke') {
    const from = typeof n.payload.from_username === 'string' ? n.payload.from_username : '?'
    const text = typeof n.payload.text === 'string' ? n.payload.text : ''
    return `${from}：${text}`
  }
  return n.type
}
</script>

<template>
  <q-btn flat round dense class="q-mr-xs" icon="notifications">
    <q-badge v-if="unread > 0" color="negative" floating rounded>{{ unread }}</q-badge>
    <q-tooltip>{{ t('notifications.bell') }}</q-tooltip>

    <q-menu anchor="bottom right" self="top right" class="nb-menu">
      <div class="nb-header row items-center">
        <span class="prts-label">{{ t('notifications.title') }}</span>
        <q-space />
        <q-btn
          v-if="items.length"
          flat
          dense
          no-caps
          size="sm"
          color="primary"
          :label="t('notifications.markAllRead')"
          @click="markAllRead"
        />
      </div>
      <q-separator />
      <q-list v-if="items.length" class="nb-list" separator>
        <q-item v-for="n in items" :key="n.id" :class="{ 'nb-item--unread': !n.read_at }">
          <q-item-section>
            <q-item-label lines="2">{{ summary(n) }}</q-item-label>
            <q-item-label caption>{{ relativeTime(n.created_at) }}</q-item-label>
          </q-item-section>
        </q-item>
      </q-list>
      <div v-else class="prts-empty nb-empty">{{ t('notifications.empty') }}</div>
    </q-menu>
  </q-btn>
</template>

<style scoped>
.nb-menu {
  width: 320px;
  max-width: 92vw;
}
.nb-header {
  padding: 10px 14px;
}
.nb-list {
  max-height: 360px;
  overflow: auto;
}
.nb-item--unread {
  background: var(--prts-accent-dim);
}
.nb-empty {
  padding: 28px 14px;
  text-align: center;
}
</style>

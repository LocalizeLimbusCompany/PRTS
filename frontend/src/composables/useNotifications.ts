import { ref } from 'vue'
import { Notify } from 'quasar'

import { notificationsApi, type NotificationDto } from '@/api'
import { useUserStream, type UserStreamEvent } from '@/composables/useUserStream'
import { i18n } from '@/i18n'

// 模块级单例状态：App 根部启动一次，<NotificationBell> 等组件复用同一份状态，
// 避免每个使用方各开一条连接，也省去 provide/inject 样板。
const unread = ref(0)
const items = ref<NotificationDto[]>([])

// 消费 shared user-stream：仅处理 type==='Notification'（私信由 useMessages 处理）。
// 处理器在模块加载时注册一次（单例）；因不在组件上下文中，改用 Quasar `Notify.create()`
// 弹 toast（而非依赖组件内 `useQuasar()`）。连接生命周期统一由 useUserStream 管理。
useUserStream().onEvent((msg: UserStreamEvent) => {
  if (msg.type !== 'Notification' || typeof msg.id !== 'number') return

  const payload = msg.payload ?? {}
  const notification: NotificationDto = {
    id: msg.id,
    type: msg.kind ?? '',
    payload,
    read_at: null,
    created_at: new Date().toISOString(),
  }
  items.value = [notification, ...items.value]
  unread.value++

  if (notification.type === 'poke') {
    const from = typeof payload.from_username === 'string' ? payload.from_username : '?'
    const text = typeof payload.text === 'string' ? payload.text : ''
    Notify.create({ type: 'info', message: `${from}: ${text}`, timeout: 5000 })
  } else {
    Notify.create({ type: 'info', message: i18n.global.t('notifications.received'), timeout: 3000 })
  }
})

/**
 * 通知状态与操作（未读数 / 最近列表 / 刷新 / 全部已读 / 重置）。
 *
 * 连接不再由本 composable 维护：`/ws/user` 由 [`useUserStream`] 统一负责，
 * App 根部登录后 `connect()` + `refresh()`，登出 `disconnect()` + `reset()`。
 */
export function useNotifications() {
  /** 拉取未读数 + 最近通知列表（登录初次与重连兜底均调用）。 */
  async function refresh() {
    try {
      const [{ count }, list] = await Promise.all([
        notificationsApi.unreadCount(),
        notificationsApi.list(),
      ])
      unread.value = count
      items.value = list
    } catch {
      /* 静默降级：铃铛不阻塞主流程 */
    }
  }

  /** 全部标记已读：调用接口 + 本地清零/同步 read_at。 */
  async function markAllRead() {
    await notificationsApi.markRead()
    unread.value = 0
    const now = new Date().toISOString()
    items.value = items.value.map((n) => (n.read_at ? n : { ...n, read_at: now }))
  }

  /** 登出重置本地状态（连接由 useUserStream.disconnect 关闭）。 */
  function reset() {
    unread.value = 0
    items.value = []
  }

  return { unread, items, refresh, markAllRead, reset }
}

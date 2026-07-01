import { ref } from 'vue'
import { useQuasar } from 'quasar'

import { getAccessToken } from '@/api/session'
import { notificationsApi, type NotificationDto } from '@/api'

/** `/ws/user` 推来的用户事件（当前仅 Notification 一种）。 */
interface UserWsMessage {
  type: string
  id?: number
  kind?: string
  payload?: Record<string, unknown>
}

// 模块级单例状态：App 根部启动一次，<NotificationBell> 等组件复用同一份状态，
// 避免每个使用方各开一条 WS 连接，也省去 provide/inject 样板。
const unread = ref(0)
const items = ref<NotificationDto[]>([])

let ws: WebSocket | null = null
let manualClose = false
let reconnectTimer: ReturnType<typeof setTimeout> | undefined
let started = false

/**
 * 连接个人通知 WebSocket（`/ws/user`），维护未读数与最近通知列表；
 * 收到新通知时弹出 toast 提醒。镜像 `useRealtime.ts` 的连接/重连写法。
 * 应在 App 根部登录后调用一次（`connect()`），登出时 `disconnect()`。
 */
export function useNotifications() {
  const $q = useQuasar()

  function connect() {
    if (started) return // 已连接（模块级单例，避免重复开连接）
    const token = getAccessToken()
    if (!token) return
    started = true
    manualClose = false
    void refresh()
    open()
  }

  function open() {
    const token = getAccessToken()
    if (!token) return
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const url = `${proto}://${location.host}/ws/user?token=${encodeURIComponent(token)}`
    ws = new WebSocket(url)
    ws.onmessage = (ev) => handle(String(ev.data))
    ws.onclose = () => {
      if (!manualClose) scheduleReconnect()
    }
  }

  function scheduleReconnect() {
    clearTimeout(reconnectTimer)
    reconnectTimer = setTimeout(open, 3000)
  }

  function handle(data: string) {
    let msg: UserWsMessage
    try {
      msg = JSON.parse(data) as UserWsMessage
    } catch {
      return
    }
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
      $q.notify({ type: 'info', message: `${from}: ${text}`, timeout: 5000 })
    } else {
      $q.notify({ type: 'info', message: '收到新通知', timeout: 3000 })
    }
  }

  /** 拉取未读数 + 最近通知列表（初次连接与重连兜底均调用）。 */
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

  function disconnect() {
    manualClose = true
    started = false
    clearTimeout(reconnectTimer)
    ws?.close()
    ws = null
    unread.value = 0
    items.value = []
  }

  return { unread, items, connect, refresh, markAllRead, disconnect }
}

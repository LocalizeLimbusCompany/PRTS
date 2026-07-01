import { getAccessToken } from '@/api/session'

/**
 * `/ws/user` 推来的用户事件（shared stream 按 `type` 分发给各订阅者）。
 *
 * 两种事件复用同一条连接（后端 `UserEvent`，`#[serde(tag="type")]`）：
 * - `Notification`：`{ type, id, kind, payload }` —— 由 `useNotifications` 消费。
 * - `DmMessage`：`{ type, id, from_user_id, content, created_at }` —— 由 `useMessages` 消费。
 */
export interface UserStreamEvent {
  type: string
  // —— Notification ——
  id?: number
  kind?: string
  payload?: Record<string, unknown>
  // —— DmMessage ——
  from_user_id?: number
  content?: string
  created_at?: string
}

type Handler = (msg: UserStreamEvent) => void

// 模块级单例：全 App 仅维护一条 /ws/user 连接，收到的事件按 type 分发给所有订阅者
// （通知 + 私信）。取代 Spec C 中 useNotifications 各自开连接的做法，避免重复连接。
const handlers = new Set<Handler>()
let ws: WebSocket | null = null
let manualClose = false
let reconnectTimer: ReturnType<typeof setTimeout> | undefined
let started = false

/** 建立连接：镜像 `useRealtime.ts` 的连接/重连写法。 */
function open() {
  const token = getAccessToken()
  if (!token) return
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const url = `${proto}://${location.host}/ws/user?token=${encodeURIComponent(token)}`
  ws = new WebSocket(url)
  ws.onmessage = (ev) => dispatch(String(ev.data))
  ws.onclose = () => {
    if (!manualClose) scheduleReconnect()
  }
}

function scheduleReconnect() {
  clearTimeout(reconnectTimer)
  reconnectTimer = setTimeout(open, 3000)
}

/** 解析一条 wire JSON，广播给所有订阅者（各自按 type 过滤）。 */
function dispatch(data: string) {
  let msg: UserStreamEvent
  try {
    msg = JSON.parse(data) as UserStreamEvent
  } catch {
    return
  }
  for (const h of handlers) {
    try {
      h(msg)
    } catch {
      /* 单个订阅者异常不影响其它订阅者 */
    }
  }
}

/**
 * 共享用户事件流（单一 `/ws/user` 连接）。
 *
 * - `onEvent(handler)`：订阅事件，返回取消订阅函数（组件卸载时调用）；
 * - `connect()`：登录后在 App 根部调用一次（幂等，取不到 token 直接返回）；
 * - `disconnect()`：登出时断开并停止重连。
 */
export function useUserStream() {
  function connect() {
    if (started) return // 已连接（模块级单例，避免重复开连接）
    const token = getAccessToken()
    if (!token) return
    started = true
    manualClose = false
    open()
  }

  function disconnect() {
    manualClose = true
    started = false
    clearTimeout(reconnectTimer)
    ws?.close()
    ws = null
  }

  function onEvent(handler: Handler): () => void {
    handlers.add(handler)
    return () => {
      handlers.delete(handler)
    }
  }

  return { connect, disconnect, onEvent }
}

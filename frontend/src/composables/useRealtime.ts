import { onUnmounted, ref } from 'vue'

import { getAccessToken } from '@/api/session'

interface RtMessage {
  type: string
  user_id?: number
  online?: number[]
  entry_id?: number
  version?: number
  by?: number
}

export interface RealtimeOptions {
  /** 他人更新了某词条时回调（已过滤自身操作）。 */
  onEntryUpdated?: (entryId: number, version: number, by: number) => void
}

/**
 * 连接项目实时房间（WebSocket）。返回在线用户、正在编辑映射与「正在编辑」上报方法。
 * 自动重连；组件卸载时断开。
 */
export function useRealtime(projectId: number, opts: RealtimeOptions = {}) {
  const online = ref<number[]>([])
  const editing = ref<Record<number, number>>({}) // entry_id -> user_id

  let ws: WebSocket | null = null
  let manualClose = false
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined
  const editingTimers = new Map<number, ReturnType<typeof setTimeout>>()

  function connect() {
    const token = getAccessToken()
    if (!token) return
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const url = `${proto}://${location.host}/ws/projects/${projectId}?token=${encodeURIComponent(token)}`
    ws = new WebSocket(url)
    ws.onmessage = (ev) => handle(String(ev.data))
    ws.onclose = () => {
      if (!manualClose) scheduleReconnect()
    }
  }

  function scheduleReconnect() {
    clearTimeout(reconnectTimer)
    reconnectTimer = setTimeout(connect, 3000)
  }

  function handle(data: string) {
    let msg: RtMessage
    try {
      msg = JSON.parse(data) as RtMessage
    } catch {
      return
    }
    switch (msg.type) {
      case 'join':
        if (Array.isArray(msg.online)) online.value = msg.online
        break
      case 'leave':
        if (typeof msg.user_id === 'number') {
          online.value = online.value.filter((u) => u !== msg.user_id)
        }
        break
      case 'editing':
        if (typeof msg.entry_id === 'number' && typeof msg.user_id === 'number') {
          const id = msg.entry_id
          editing.value = { ...editing.value, [id]: msg.user_id }
          const t = editingTimers.get(id)
          if (t) clearTimeout(t)
          editingTimers.set(
            id,
            setTimeout(() => {
              const next = { ...editing.value }
              delete next[id]
              editing.value = next
              editingTimers.delete(id)
            }, 6000),
          )
        }
        break
      case 'entry_updated':
        if (
          typeof msg.entry_id === 'number' &&
          typeof msg.version === 'number' &&
          typeof msg.by === 'number'
        ) {
          opts.onEntryUpdated?.(msg.entry_id, msg.version, msg.by)
        }
        break
    }
  }

  /** 上报「我正在编辑某词条」。 */
  function sendEditing(entryId: number) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'editing', entry_id: entryId }))
    }
  }

  function disconnect() {
    manualClose = true
    clearTimeout(reconnectTimer)
    editingTimers.forEach((t) => clearTimeout(t))
    ws?.close()
    ws = null
  }

  connect()
  onUnmounted(disconnect)

  return { online, editing, sendEditing, disconnect }
}

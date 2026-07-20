import { computed, onUnmounted, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { getAccessToken } from '@/api/session'

interface RtMessage {
  type: string
  user_id?: number
  online?: number[]
  entry_id?: number
  version?: number
  by?: number
  presences?: PresenceState[]
}

export interface PresenceState {
  connection_id: string
  user_id: number
  file_id: number | null
  entry_id: number | null
}

export interface RealtimeOptions {
  /** 他人更新了某词条时回调（已过滤自身操作）。 */
  onEntryUpdated?: (entryId: number, version: number, by: number) => void
  /** 当前词条评论发生变化。 */
  onEntryCommentChanged?: (entryId: number, by: number) => void
}

export function shouldConnectProjectRealtime(
  authenticated: boolean,
  collaborate: boolean,
): boolean {
  return authenticated && collaborate
}

export function canOpenPresenceMenu(
  presenceUserId: number,
  currentUserId: number | null | undefined,
  collaborate: boolean,
): boolean {
  return collaborate && currentUserId != null && presenceUserId !== currentUserId
}

/**
 * 连接项目实时房间（WebSocket）。返回在线用户、正在编辑映射与「正在编辑」上报方法。
 * 自动重连；组件卸载时断开。
 */
export function useRealtime(
  projectId: MaybeRefOrGetter<number>,
  opts: RealtimeOptions = {},
  enabled: MaybeRefOrGetter<boolean> = true,
) {
  const presences = ref<PresenceState[]>([])
  const online = computed(() => [...new Set(presences.value.map((item) => item.user_id))])
  const editing = computed<Record<number, number[]>>(() => {
    const result: Record<number, number[]> = {}
    for (const presence of presences.value) {
      if (presence.entry_id == null) continue
      const users = result[presence.entry_id] ?? []
      if (!users.includes(presence.user_id)) users.push(presence.user_id)
      result[presence.entry_id] = users
    }
    return result
  })

  let ws: WebSocket | null = null
  let active = false
  let manualClose = false
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined
  let heartbeatTimer: ReturnType<typeof setInterval> | undefined
  let currentEntryId: number | null = null
  let currentFileId: number | null = null

  function connect() {
    if (!active || ws) return
    const token = getAccessToken()
    if (!token) return
    const currentProjectId = toValue(projectId)
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const url = `${proto}://${location.host}/ws/projects/${currentProjectId}?token=${encodeURIComponent(token)}`
    const socket = new WebSocket(url)
    ws = socket
    socket.onopen = () => {
      if (ws !== socket) return
      sendPresence()
      clearInterval(heartbeatTimer)
      // Re-publish the full state instead of only extending TTL. A lease lost during a brief
      // Redis/network interruption is therefore recreated by the next heartbeat.
      heartbeatTimer = setInterval(sendPresence, 10_000)
    }
    socket.onmessage = (ev) => {
      if (ws === socket) handle(String(ev.data))
    }
    socket.onclose = () => {
      if (ws !== socket) return
      ws = null
      presences.value = []
      clearInterval(heartbeatTimer)
      if (!manualClose) scheduleReconnect()
    }
  }

  function scheduleReconnect() {
    if (!active) return
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
        break
      case 'leave':
        if (typeof msg.user_id === 'number') {
          presences.value = presences.value.filter((item) => item.user_id !== msg.user_id)
        }
        break
      case 'editing':
        if (typeof msg.entry_id === 'number' && typeof msg.user_id === 'number') {
          // 旧服务端兼容事件；权威状态由 presence_snapshot 覆盖。
        }
        break
      case 'presence_snapshot':
        if (Array.isArray(msg.presences)) presences.value = msg.presences
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
      case 'entry_comment_changed':
        if (typeof msg.entry_id === 'number' && typeof msg.by === 'number') {
          opts.onEntryCommentChanged?.(msg.entry_id, msg.by)
        }
        break
    }
  }

  /** 上报「我正在编辑某词条」。 */
  function sendEditing(entryId: number) {
    currentEntryId = entryId
    sendPresence()
  }

  /** 上报当前浏览文件；用于文件在线人数，不占用任何词条。 */
  function sendViewing(fileId: number) {
    currentFileId = fileId
    currentEntryId = null
    sendPresence()
  }

  /** 清除当前连接的词条占用，但保持项目在线。 */
  function sendIdle() {
    currentFileId = null
    currentEntryId = null
    sendPresence()
  }

  function sendPresence() {
    if (currentEntryId != null) send({ type: 'editing', entry_id: currentEntryId })
    else if (currentFileId != null) send({ type: 'viewing', file_id: currentFileId })
    else send({ type: 'idle' })
  }

  function send(payload: Record<string, unknown>) {
    if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(payload))
  }

  function disconnect() {
    manualClose = true
    clearTimeout(reconnectTimer)
    clearInterval(heartbeatTimer)
    const socket = ws
    ws = null
    presences.value = []
    socket?.close()
  }

  watch(
    () => [toValue(enabled), toValue(projectId)] as const,
    ([value]) => {
      disconnect()
      active = value
      if (active) {
        manualClose = false
        connect()
      }
    },
    { immediate: true },
  )
  onUnmounted(disconnect)

  return { online, presences, editing, sendEditing, sendViewing, sendIdle, disconnect }
}

import { ref } from 'vue'
import { Notify } from 'quasar'

import { messagesApi, type ThreadDto } from '@/api'
import { useUserStream, type UserStreamEvent } from '@/composables/useUserStream'

// 模块级单例：会话列表 + 未读总数。App 根部登录后 refresh()，登出 reset()。
const threads = ref<ThreadDto[]>([])
const unread = ref(0)

// 当前正在查看的会话「对方 id」（由 MessageThreadView 挂载/卸载时设置）。
// 用于避免在用户正看着的会话上重复计未读 / 弹 toast（该会话由会话页自行处理）。
let activeThreadUserId: number | null = null

/** 拉取会话列表（收到新私信、进入列表页时刷新顺序/最后一条/未读徽标）。 */
async function refreshThreads() {
  try {
    threads.value = await messagesApi.threads()
  } catch {
    /* 静默降级：私信不阻塞主流程 */
  }
}

// 消费 shared user-stream：仅处理 type==='DmMessage'（通知由 useNotifications 处理）。
// 模块加载时注册一次（单例）；toast 用 Quasar Notify.create（无需组件上下文）。
useUserStream().onEvent((msg: UserStreamEvent) => {
  if (msg.type !== 'DmMessage' || typeof msg.from_user_id !== 'number') return
  const from = msg.from_user_id

  // 正在查看该会话：会话页会自行追加消息并标记已读，这里不计未读、不弹 toast。
  if (from === activeThreadUserId) return

  unread.value++
  // 尽力从现有会话取对方用户名做 toast 抬头（新会话可能取不到，退化为纯内容）。
  const name = threads.value.find((t) => t.other_user_id === from)?.username
  const preview = (typeof msg.content === 'string' ? msg.content : '').slice(0, 40)
  Notify.create({
    type: 'info',
    message: name ? `${name}: ${preview}` : preview || '收到新私信',
    timeout: 4000,
  })
  // 刷新会话列表以更新该会话的最后一条 / 排序 / 未读徽标。
  void refreshThreads()
})

/**
 * 私信状态与操作（会话列表 / 未读总数 / 刷新 / 重置 / 标记当前会话）。
 *
 * 连接由 [`useUserStream`] 统一维护；本 composable 只持有状态并消费 `DmMessage`。
 */
export function useMessages() {
  /** 拉取会话列表 + 未读总数（登录初次、进入列表页兜底调用）。 */
  async function refresh() {
    try {
      const [list, { count }] = await Promise.all([
        messagesApi.threads(),
        messagesApi.unreadCount(),
      ])
      threads.value = list
      unread.value = count
    } catch {
      /* 静默降级 */
    }
  }

  /** 登出重置本地状态。 */
  function reset() {
    threads.value = []
    unread.value = 0
    activeThreadUserId = null
  }

  /**
   * 由会话页设置「当前正在查看的会话对方 id」；传 `null` 表示离开会话页。
   * 设置后，来自该对方的 `DmMessage` 不再计未读 / 弹 toast（由会话页即时追加处理）。
   */
  function setActiveThread(userId: number | null) {
    activeThreadUserId = userId
  }

  return { threads, unread, refresh, reset, setActiveThread }
}

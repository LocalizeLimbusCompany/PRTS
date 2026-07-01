<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import { useAuthStore } from '@/stores/auth'
import { useUserStream } from '@/composables/useUserStream'
import { useNotifications } from '@/composables/useNotifications'
import NotificationBell from '@/components/NotificationBell.vue'

const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()

const initials = computed(() => auth.user?.username?.slice(0, 2).toUpperCase() ?? '')

// 实时：登录态下在 App 根部保持一条共享 /ws/user 连接（通知 + 私信共用）；登出即断开。
// ensureReady() 尚未完成时 auth.user 为 null，watch 的 immediate 调用会因此
// 跳过连接（connect() 内部也会因取不到 token 而直接返回），待恢复完成后
// user 变化再次触发即可正常连上，无需额外等待。
const userStream = useUserStream()
const notifications = useNotifications()
watch(
  () => auth.user,
  (user) => {
    if (user) {
      userStream.connect()
      void notifications.refresh()
    } else {
      userStream.disconnect()
      notifications.reset()
    }
  },
  { immediate: true },
)

function toggleTheme() {
  $q.dark.toggle()
  localStorage.setItem('prts_theme', $q.dark.isActive ? 'dark' : 'light')
}

async function logout() {
  await auth.logout()
  router.push({ name: 'login' })
}
</script>

<template>
  <q-layout view="hHh lpR fFf">
    <q-header class="prts-header">
      <q-toolbar class="prts-toolbar">
        <router-link to="/projects" class="prts-brand">
          <span class="prts-brand__mark">PRTS</span>
          <span class="prts-brand__sub">L10N&nbsp;TERMINAL</span>
        </router-link>

        <q-btn
          flat
          no-caps
          dense
          class="prts-navbtn q-ml-md"
          :to="{ name: 'projects' }"
          label="项目"
        />
        <q-btn
          v-if="auth.isAdmin"
          flat
          no-caps
          dense
          class="prts-navbtn"
          :to="{ name: 'admin' }"
          label="管理"
        />

        <q-space />

        <NotificationBell v-if="auth.isAuthed" />

        <q-btn
          flat
          round
          dense
          class="q-mr-xs"
          :icon="$q.dark.isActive ? 'dark_mode' : 'light_mode'"
          @click="toggleTheme"
        >
          <q-tooltip>切换{{ $q.dark.isActive ? '浅色' : '深色' }}主题</q-tooltip>
        </q-btn>

        <template v-if="auth.isAuthed">
          <q-btn-dropdown flat no-caps class="prts-userbtn">
            <template #label>
              <q-avatar size="24px" class="prts-userbtn__avatar">
                <img v-if="auth.user?.avatar_url" :src="auth.user.avatar_url" alt="" />
                <span v-else>{{ initials }}</span>
              </q-avatar>
              <span class="q-ml-sm prts-username">{{ auth.user?.username }}</span>
            </template>
            <q-list style="min-width: 168px">
              <q-item v-close-popup clickable :to="{ name: 'me' }">
                <q-item-section avatar><q-icon name="person" /></q-item-section>
                <q-item-section>个人主页</q-item-section>
              </q-item>
              <q-item v-if="auth.isAdmin" v-close-popup clickable :to="{ name: 'admin' }">
                <q-item-section avatar><q-icon name="shield" /></q-item-section>
                <q-item-section>管理后台</q-item-section>
              </q-item>
              <q-separator />
              <q-item v-close-popup clickable @click="logout">
                <q-item-section avatar><q-icon name="logout" color="negative" /></q-item-section>
                <q-item-section class="text-negative">登出</q-item-section>
              </q-item>
            </q-list>
          </q-btn-dropdown>
        </template>
        <template v-else>
          <q-btn flat no-caps :to="{ name: 'login' }" label="登录" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            class="q-ml-sm"
            :to="{ name: 'register' }"
            label="注册"
          />
        </template>
      </q-toolbar>
    </q-header>

    <q-page-container>
      <router-view v-slot="{ Component }">
        <transition name="prts-fade" mode="out-in">
          <component :is="Component" />
        </transition>
      </router-view>
    </q-page-container>
  </q-layout>
</template>

<style scoped>
.prts-header {
  background: rgba(11, 14, 20, 0.82);
  backdrop-filter: blur(10px);
  border-bottom: 1px solid var(--prts-border);
  box-shadow: 0 1px 0 rgba(54, 197, 208, 0.12);
}
.prts-toolbar {
  height: var(--prts-nav-h);
  max-width: 1440px;
  margin: 0 auto;
  width: 100%;
  padding: 0 18px;
}
.prts-brand {
  display: flex;
  flex-direction: column;
  line-height: 1;
}
.prts-brand__mark {
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 20px;
  letter-spacing: 0.14em;
  color: var(--prts-text-strong);
}
.prts-brand__mark::before {
  content: '▍';
  color: var(--prts-accent);
  margin-right: 4px;
}
.prts-brand__sub {
  font-family: var(--font-mono);
  font-size: 8.5px;
  letter-spacing: 0.34em;
  color: var(--prts-accent);
  margin-top: 3px;
}
.prts-navbtn {
  font-size: 12px;
  color: var(--prts-text-dim);
}
.prts-navbtn.router-link-active {
  color: var(--prts-accent);
}
.prts-userbtn__avatar {
  background: var(--prts-accent-dim);
  color: var(--prts-accent-strong);
  font-family: var(--font-mono);
  font-size: 11px;
}

@media (max-width: 599px) {
  .prts-toolbar {
    padding: 0 10px;
  }
  .prts-brand__sub {
    display: none;
  }
  .prts-username {
    display: none;
  }
}
</style>

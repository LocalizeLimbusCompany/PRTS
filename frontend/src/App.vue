<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { useAuthStore } from '@/stores/auth'
import { useUserStream } from '@/composables/useUserStream'
import { useNotifications } from '@/composables/useNotifications'
import { useMessages } from '@/composables/useMessages'
import NotificationBell from '@/components/NotificationBell.vue'

const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()
const { t } = useI18n()

const initials = computed(() => auth.user?.username?.slice(0, 2).toUpperCase() ?? '')

// 实时：登录态下在 App 根部保持一条共享 /ws/user 连接（通知 + 私信共用）；登出即断开。
// ensureReady() 尚未完成时 auth.user 为 null，watch 的 immediate 调用会因此
// 跳过连接（connect() 内部也会因取不到 token 而直接返回），待恢复完成后
// user 变化再次触发即可正常连上，无需额外等待。
const userStream = useUserStream()
const notifications = useNotifications()
const { unread: messagesUnread, refresh: refreshMessages, reset: resetMessages } = useMessages()
watch(
  () => auth.user,
  (user) => {
    if (user) {
      userStream.connect()
      void notifications.refresh()
      void refreshMessages()
    } else {
      userStream.disconnect()
      notifications.reset()
      resetMessages()
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
          :label="t('app.projects')"
        />
        <q-btn
          flat
          no-caps
          dense
          class="prts-navbtn"
          :to="{ name: 'platform-leaderboard' }"
          :label="t('app.leaderboard')"
        />
        <q-btn
          v-if="auth.isAdmin"
          flat
          no-caps
          dense
          class="prts-navbtn"
          :to="{ name: 'admin' }"
          :label="t('app.admin')"
        />

        <q-space />

        <NotificationBell v-if="auth.isAuthed" />

        <q-btn
          v-if="auth.isAuthed"
          flat
          round
          dense
          class="q-mr-xs"
          icon="mdi-email-outline"
          :to="{ name: 'messages' }"
        >
          <q-badge v-if="messagesUnread > 0" color="negative" floating rounded>
            {{ messagesUnread }}
          </q-badge>
          <q-tooltip>{{ t('app.messages') }}</q-tooltip>
        </q-btn>

        <q-btn
          flat
          round
          dense
          class="q-mr-xs"
          :icon="$q.dark.isActive ? 'mdi-weather-night' : 'mdi-white-balance-sunny'"
          @click="toggleTheme"
        >
          <q-tooltip>{{
            t($q.dark.isActive ? 'app.switchToLightTheme' : 'app.switchToDarkTheme')
          }}</q-tooltip>
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
                <q-item-section avatar><q-icon name="mdi-account-outline" /></q-item-section>
                <q-item-section>{{ t('app.profile') }}</q-item-section>
              </q-item>
              <q-item v-close-popup clickable :to="{ name: 'messages' }">
                <q-item-section avatar><q-icon name="mdi-email-outline" /></q-item-section>
                <q-item-section>{{ t('app.messages') }}</q-item-section>
                <q-item-section v-if="messagesUnread > 0" side>
                  <q-badge color="negative" rounded>{{ messagesUnread }}</q-badge>
                </q-item-section>
              </q-item>
              <q-item v-if="auth.isAdmin" v-close-popup clickable :to="{ name: 'admin' }">
                <q-item-section avatar><q-icon name="mdi-shield-outline" /></q-item-section>
                <q-item-section>{{ t('app.adminPanel') }}</q-item-section>
              </q-item>
              <q-separator />
              <q-item v-close-popup clickable @click="logout">
                <q-item-section avatar
                  ><q-icon name="mdi-logout" color="negative"
                /></q-item-section>
                <q-item-section class="text-negative">{{ t('app.logout') }}</q-item-section>
              </q-item>
            </q-list>
          </q-btn-dropdown>
        </template>
        <template v-else>
          <q-btn flat no-caps :to="{ name: 'login' }" :label="t('app.login')" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            class="q-ml-sm"
            :to="{ name: 'register' }"
            :label="t('app.register')"
          />
        </template>
      </q-toolbar>
    </q-header>

    <q-page-container>
      <q-banner
        v-if="auth.passwordChangeRequired"
        class="bg-warning text-dark"
        inline-actions
        rounded
      >
        {{ t('app.passwordChangeRequired') }}
        <template #action>
          <q-btn flat no-caps :label="t('app.changePassword')" :to="{ name: 'me' }" />
        </template>
      </q-banner>
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
  padding: 0 18px 0 0;
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
    padding: 0 10px 0 0;
  }
  .prts-brand__sub {
    display: none;
  }
  .prts-username {
    display: none;
  }
}
</style>

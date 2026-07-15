import { createRouter, createWebHashHistory } from 'vue-router'

import { useAuthStore } from '@/stores/auth'

import { projectRoutes } from './projectRoutes'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/projects' },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { guestOnly: true },
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/RegisterView.vue'),
      meta: { guestOnly: true },
    },
    {
      // ZOOT 等 OAuth 回调：后端跳转至 /#/oauth?access_token=...&refresh_token=...
      path: '/oauth',
      name: 'oauth',
      component: () => import('@/views/OAuthCallbackView.vue'),
    },
    {
      path: '/projects',
      name: 'projects',
      component: () => import('@/views/ProjectsView.vue'),
    },
    {
      path: '/leaderboard',
      name: 'platform-leaderboard',
      component: () => import('@/views/PlatformLeaderboardView.vue'),
    },
    ...projectRoutes,
    {
      path: '/me',
      name: 'me',
      component: () => import('@/views/ProfileView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/messages',
      name: 'messages',
      component: () => import('@/views/MessagesView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/messages/:userId(\\d+)',
      name: 'message-thread',
      component: () => import('@/views/MessageThreadView.vue'),
      props: (route) => ({ userId: Number(route.params.userId) }),
      meta: { requiresAuth: true },
    },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminView.vue'),
      meta: { requiresAuth: true, adminOnly: true },
    },
    { path: '/:pathMatch(.*)*', redirect: '/projects' },
  ],
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  await auth.ensureReady()

  if (to.meta.requiresAuth && !auth.isAuthed) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
  if (to.meta.adminOnly && !auth.isAdmin) {
    return { name: 'projects' }
  }
  if (to.meta.guestOnly && auth.isAuthed) {
    return { name: 'projects' }
  }
  return true
})

export default router

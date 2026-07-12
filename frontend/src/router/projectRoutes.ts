import type { RouteRecordRaw } from 'vue-router'

/** Project workspace routes keep the editor outside the nested shell. */
export const projectRoutes: RouteRecordRaw[] = [
  {
    path: '/projects/:id(\\d+)',
    component: () => import('@/views/ProjectDetailView.vue'),
    props: (route) => ({ id: Number(route.params.id) }),
    children: [
      { path: '', redirect: { name: 'project-info' } },
      {
        path: 'info',
        name: 'project-info',
        component: () => import('@/views/project/ProjectInfoView.vue'),
      },
      {
        path: 'files',
        name: 'project-files',
        component: () => import('@/views/project/ProjectFilesView.vue'),
      },
      {
        path: 'leaderboard',
        name: 'project-leaderboard',
        component: () => import('@/views/project/ProjectLeaderboardView.vue'),
      },
      {
        path: 'download',
        name: 'project-download',
        component: () => import('@/views/project/ProjectDownloadView.vue'),
      },
      {
        path: 'manage',
        name: 'project-manage',
        component: () => import('@/views/project/ProjectManageView.vue'),
      },
    ],
  },
  {
    path: '/projects/:id(\\d+)/editor',
    name: 'editor',
    component: () => import('@/views/EditorView.vue'),
    props: (route) => ({ id: Number(route.params.id) }),
    meta: { requiresAuth: true },
  },
]

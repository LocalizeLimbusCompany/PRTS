import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'home',
      component: () => import('@/views/HomeView.vue'),
    },
    // 后续阶段在此挂载：项目列表、项目详情、翻译编辑器、用户主页、管理后台等。
  ],
})

export default router

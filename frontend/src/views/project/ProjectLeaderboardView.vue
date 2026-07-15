<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import { apiErrorMessage, leaderboardsApi, type LeaderboardEntryDto } from '@/api'
import LeaderboardTable from '@/components/LeaderboardTable.vue'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { projectId } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const items = ref<LeaderboardEntryDto[]>([])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    items.value = (await leaderboardsApi.project(projectId.value)).items
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('leaderboard.loadFailed')) })
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(projectId, load)
</script>

<template>
  <section>
    <header class="project-leaderboard__header">
      <div class="prts-label">{{ t('project.sections.leaderboard') }}</div>
      <h2 class="prts-h2">{{ t('project.leaderboard.heading') }}</h2>
      <p class="prts-dim">{{ t('project.leaderboard.description') }}</p>
    </header>
    <LeaderboardTable :items="items" :loading="loading" />
  </section>
</template>

<style scoped>
.project-leaderboard__header {
  margin-bottom: 20px;
}

.project-leaderboard__header h2 {
  margin: 6px 0;
}

.project-leaderboard__header p {
  margin: 0;
}
</style>

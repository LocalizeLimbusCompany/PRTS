<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import {
  apiErrorMessage,
  leaderboardsApi,
  type LeaderboardPeriod,
  type LeaderboardResponse,
} from '@/api'
import LeaderboardTable from '@/components/LeaderboardTable.vue'

const $q = useQuasar()
const { t } = useI18n()
const period = ref<LeaderboardPeriod>('all')
const response = ref<LeaderboardResponse | null>(null)
const loading = ref(false)

const options = computed<Array<{ label: string; value: LeaderboardPeriod }>>(() => [
  { label: t('leaderboard.all'), value: 'all' },
  { label: t('leaderboard.month'), value: 'month' },
  { label: t('leaderboard.week'), value: 'week' },
])

async function load() {
  loading.value = true
  try {
    response.value = await leaderboardsApi.platform(period.value)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('leaderboard.loadFailed')) })
  } finally {
    loading.value = false
  }
}

function formatUtcDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeZone: 'UTC' }).format(
    new Date(value),
  )
}

onMounted(load)
</script>

<template>
  <q-page class="prts-container prts-container--narrow">
    <header class="leaderboard-header">
      <div>
        <div class="prts-label">// PLATFORM CP</div>
        <h1 class="prts-h1">{{ t('leaderboard.platformTitle') }}</h1>
        <p class="prts-dim">{{ t('leaderboard.platformDescription') }}</p>
      </div>
      <q-btn-toggle
        v-model="period"
        no-caps
        unelevated
        toggle-color="primary"
        toggle-text-color="dark"
        :options="options"
        @update:model-value="load"
      />
    </header>
    <div v-if="response?.period_start && response.period_end" class="prts-dim q-mb-md">
      {{
        t('leaderboard.utcPeriod', {
          start: formatUtcDate(response.period_start),
          end: formatUtcDate(response.period_end),
        })
      }}
    </div>
    <LeaderboardTable :items="response?.items ?? []" :loading="loading" />
  </q-page>
</template>

<style scoped>
.leaderboard-header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 24px;
  margin-bottom: 20px;
}

.leaderboard-header p {
  margin: 8px 0 0;
}

@media (max-width: 720px) {
  .leaderboard-header {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>

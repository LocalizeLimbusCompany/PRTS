<script setup lang="ts">
import type { LeaderboardEntryDto } from '@/api'

defineProps<{
  items: LeaderboardEntryDto[]
  loading?: boolean
}>()

/** CP wire value uses exact tenths; retain one decimal only when needed. */
function formatCp(tenths: number): string {
  const value = tenths / 10
  return Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1)
}
</script>

<template>
  <q-card flat bordered>
    <q-inner-loading :showing="Boolean(loading)" />
    <q-markup-table flat separator="horizontal">
      <thead>
        <tr>
          <th class="text-left">{{ $t('leaderboard.rank') }}</th>
          <th class="text-left">{{ $t('leaderboard.contributor') }}</th>
          <th class="text-right">{{ $t('leaderboard.cp') }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="item in items" :key="item.user_id">
          <td class="leaderboard-rank prts-mono">#{{ item.rank }}</td>
          <td>
            <div class="row items-center no-wrap q-gutter-sm">
              <q-avatar square size="32px" color="primary" text-color="dark">
                <img v-if="item.avatar_url" :src="item.avatar_url" alt="" />
                <span v-else>{{ item.username.slice(0, 2).toUpperCase() }}</span>
              </q-avatar>
              <span>{{ item.username }}</span>
            </div>
          </td>
          <td class="text-right text-accent prts-mono">{{ formatCp(item.cp_tenths) }}</td>
        </tr>
        <tr v-if="!loading && items.length === 0">
          <td colspan="3" class="prts-empty">{{ $t('leaderboard.empty') }}</td>
        </tr>
      </tbody>
    </q-markup-table>
  </q-card>
</template>

<style scoped>
.leaderboard-rank {
  width: 84px;
  color: var(--prts-text-dim);
}

th,
td {
  height: 54px;
}
</style>

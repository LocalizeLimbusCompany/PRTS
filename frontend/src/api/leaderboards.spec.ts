// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'

import { http, leaderboardsApi } from '@/api'
import leaderboardTableSource from '@/components/LeaderboardTable.vue?raw'
import platformViewSource from '@/views/PlatformLeaderboardView.vue?raw'
import profileSource from '@/views/ProfileView.vue?raw'
import projectViewSource from '@/views/project/ProjectLeaderboardView.vue?raw'

afterEach(() => vi.restoreAllMocks())

describe('contribution leaderboards', () => {
  it('calls explicit platform periods and the project all-time endpoint', async () => {
    const response = { period: 'all', period_start: null, period_end: null, items: [] }
    const get = vi.spyOn(http, 'get').mockResolvedValue({ data: response })

    await leaderboardsApi.platform('week')
    await leaderboardsApi.platform('month')
    await leaderboardsApi.platform('all')
    await leaderboardsApi.project(42)

    expect(get).toHaveBeenNthCalledWith(1, '/leaderboards/platform', {
      params: { period: 'week' },
    })
    expect(get).toHaveBeenNthCalledWith(2, '/leaderboards/platform', {
      params: { period: 'month' },
    })
    expect(get).toHaveBeenNthCalledWith(3, '/leaderboards/platform', {
      params: { period: 'all' },
    })
    expect(get).toHaveBeenNthCalledWith(4, '/projects/42/leaderboard')
  })

  it('renders real exact-tenths CP instead of the former placeholder', () => {
    expect(projectViewSource).toContain('leaderboardsApi.project')
    expect(projectViewSource).not.toContain('leaderboard-placeholder')
    expect(platformViewSource).toContain("'all'")
    expect(platformViewSource).toContain("'month'")
    expect(platformViewSource).toContain("'week'")
    expect(platformViewSource).toContain("timeZone: 'UTC'")
    expect(leaderboardTableSource).toContain('tenths / 10')
    expect(profileSource).toContain('cp_tenths')
  })
})

<template>
  <section class="leaderboard-placeholder">
    <div class="prts-label">{{ $t('project.sections.leaderboard') }}</div>
    <q-icon name="mdi-podium" size="48px" color="primary" />
    <h2>{{ $t('project.leaderboard.heading') }}</h2>
    <p>{{ $t('project.leaderboard.description') }}</p>
  </section>
</template>

<style scoped>
.leaderboard-placeholder {
  display: grid;
  place-items: center;
  min-height: 360px;
  padding: 44px;
  border: 1px dashed var(--prts-border);
  background: var(--prts-panel);
  text-align: center;
}

.leaderboard-placeholder h2 {
  margin: 4px 0 0;
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.leaderboard-placeholder p {
  max-width: 480px;
  margin: 0;
  color: var(--prts-text-dim);
}
</style>

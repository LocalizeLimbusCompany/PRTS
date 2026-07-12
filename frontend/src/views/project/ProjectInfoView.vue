<script setup lang="ts">
import { computed } from 'vue'

import MarkdownView from '@/components/MarkdownView.vue'
import ProjectAvatar from '@/components/project/ProjectAvatar.vue'
import ProjectProgress from '@/components/project/ProjectProgress.vue'
import { langLabel } from '@/lib/langs'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail } = useProjectWorkspace()
const project = computed(() => detail.value?.project)
</script>

<template>
  <section v-if="detail && project" class="project-info">
    <div class="project-info__lead">
      <div class="project-info__identity">
        <ProjectAvatar
          :project-id="project.id"
          :name="project.name"
          :avatar-url="project.avatar_url"
          :avatar-updated-at="project.avatar_updated_at"
          size="76px"
        />
        <div>
          <div class="prts-label">{{ $t('project.sections.info') }}</div>
          <h2 class="project-info__heading">{{ project.name }}</h2>
          <div class="prts-dim q-mt-xs">{{ $t('project.infoHeading') }}</div>
        </div>
      </div>
      <div class="project-info__language-flow prts-mono">
        <div>
          <span class="prts-label">{{ $t('project.sourceLanguages') }}</span>
          <div class="project-info__tags">
            <q-chip v-for="language in project.source_langs" :key="language" square dense>
              {{ langLabel(language) }} · {{ language }}
              <q-tooltip v-if="language === project.primary_source_lang">
                {{ $t('project.primarySource') }}
              </q-tooltip>
            </q-chip>
          </div>
        </div>
        <q-icon name="mdi-arrow-right" size="20px" class="prts-dim" />
        <div>
          <span class="prts-label">{{ $t('project.targetLanguage') }}</span>
          <div class="project-info__target text-accent">
            {{ langLabel(project.target_lang) }} · {{ project.target_lang }}
          </div>
        </div>
      </div>
    </div>

    <q-card flat bordered class="project-info__progress">
      <q-card-section>
        <ProjectProgress :state-counts="detail.state_counts" :total="detail.entry_count" />
      </q-card-section>
    </q-card>

    <div class="project-info__grid">
      <q-card flat bordered class="project-info__description">
        <q-card-section>
          <div class="prts-label q-mb-md">{{ $t('project.description') }}</div>
          <MarkdownView v-if="project.description" :source="project.description" />
          <div v-else class="prts-dim">{{ $t('project.noDescription') }}</div>
        </q-card-section>
      </q-card>
      <q-card flat bordered>
        <q-card-section class="project-info__facts">
          <div class="prts-label">{{ $t('project.metadata') }}</div>
          <dl>
            <div>
              <dt>{{ $t('project.slug') }}</dt>
              <dd class="prts-mono">{{ project.slug }}</dd>
            </div>
            <div>
              <dt>{{ $t('project.visibility') }}</dt>
              <dd>{{ $t(`project.${project.visibility}`) }}</dd>
            </div>
            <div>
              <dt>{{ $t('project.createdAt') }}</dt>
              <dd>{{ new Date(project.created_at).toLocaleDateString() }}</dd>
            </div>
          </dl>
        </q-card-section>
      </q-card>
    </div>
  </section>
</template>

<style scoped>
.project-info {
  display: grid;
  gap: 20px;
}

.project-info__lead,
.project-info__identity,
.project-info__language-flow,
.project-info__tags {
  display: flex;
  align-items: center;
}

.project-info__lead {
  justify-content: space-between;
  gap: 28px;
}

.project-info__identity {
  gap: 14px;
}

.project-info__heading {
  margin: 4px 0 0;
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.project-info__language-flow {
  gap: 18px;
  padding: 12px 14px;
  border-left: 2px solid var(--prts-accent);
  background: var(--prts-panel-2);
}

.project-info__tags {
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 5px;
}

.project-info__target {
  margin-top: 9px;
  white-space: nowrap;
}

.project-info__progress {
  border-top-color: var(--prts-accent);
}

.project-info__grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 260px;
  gap: 20px;
}

.project-info__description {
  min-height: 220px;
}

.project-info__facts {
  display: grid;
  gap: 12px;
}

.project-info__facts dl,
.project-info__facts dd {
  margin: 0;
}

.project-info__facts dl {
  display: grid;
  gap: 14px;
}

.project-info__facts dl > div {
  padding-bottom: 12px;
  border-bottom: 1px solid var(--prts-border-soft);
}

.project-info__facts dt {
  margin-bottom: 3px;
  color: var(--prts-text-dim);
  font-size: 11px;
}

.project-info__facts dd {
  color: var(--prts-text-strong);
}

@media (max-width: 960px) {
  .project-info__lead {
    align-items: stretch;
    flex-direction: column;
  }

  .project-info__grid {
    grid-template-columns: 1fr;
  }
}
</style>

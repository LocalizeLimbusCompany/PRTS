<script setup lang="ts">
import { computed, onMounted, provide, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import { apiErrorMessage, projectsApi, type ProjectDetailDto } from '@/api'
import { hasProjectCapability } from '@/lib/capabilities'
import {
  PROJECT_WORKSPACE_SECTIONS,
  projectWorkspaceKey,
  type ProjectWorkspaceContext,
} from '@/lib/projectWorkspace'

const props = defineProps<{ id: number }>()
const route = useRoute()
const router = useRouter()
const $q = useQuasar()
const { t } = useI18n()

const detail = ref<ProjectDetailDto | null>(null)
const loading = ref(true)
const projectId = computed(() => props.id)

const sections = computed(() =>
  PROJECT_WORKSPACE_SECTIONS.filter((section) => {
    if (!('capability' in section)) return true
    return hasProjectCapability(detail.value?.capabilities, section.capability)
  }),
)

/** Load the project once for all nested workspace views. */
async function load() {
  loading.value = true
  try {
    detail.value = await projectsApi.get(props.id)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.loadFailed')) })
    await router.replace({ name: 'projects' })
  } finally {
    loading.value = false
  }
}

const context: ProjectWorkspaceContext = { detail, loading, projectId, reload: load }
provide(projectWorkspaceKey, context)

function openSection(routeName: string | null) {
  if (!routeName) return
  router.push({ name: routeName, params: { id: props.id } })
}

onMounted(load)
watch(() => props.id, load)
</script>

<template>
  <q-page class="prts-container project-shell">
    <q-inner-loading :showing="loading" />
    <template v-if="detail">
      <header class="project-shell__masthead">
        <div class="project-shell__identity">
          <q-avatar square size="54px" class="project-shell__avatar">
            <img v-if="detail.project.avatar_url" :src="detail.project.avatar_url" alt="" />
            <span v-else>{{ detail.project.name.slice(0, 2).toUpperCase() }}</span>
          </q-avatar>
          <div class="project-shell__title">
            <div class="prts-label">// {{ detail.project.slug }}</div>
            <div class="row items-center q-gutter-sm">
              <h1 class="prts-h1">{{ detail.project.name }}</h1>
              <q-badge
                v-if="detail.project.visibility === 'private'"
                outline
                color="grey"
                :label="$t('project.private')"
              />
            </div>
          </div>
        </div>
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-file-edit-outline"
          :label="$t('project.openEditor')"
          :to="{ name: 'editor', params: { id: props.id } }"
        />
      </header>

      <div class="project-shell__layout">
        <aside class="project-shell__nav" :aria-label="$t('project.workspaceNav')">
          <button
            v-for="section in sections"
            :key="section.key"
            type="button"
            class="project-shell__nav-item"
            :class="{
              'project-shell__nav-item--active': section.route === route.name,
              'project-shell__nav-item--pending': 'pending' in section,
            }"
            :disabled="'pending' in section"
            @click="openSection(section.route)"
          >
            <q-icon :name="section.icon" size="18px" />
            <span>{{ $t(`project.sections.${section.key}`) }}</span>
            <span v-if="'pending' in section" class="project-shell__soon">{{ $t('project.soon') }}</span>
          </button>
        </aside>
        <main class="project-shell__content">
          <router-view />
        </main>
      </div>
    </template>
  </q-page>
</template>

<style scoped>
.project-shell {
  max-width: 1520px;
}

.project-shell__masthead,
.project-shell__identity,
.project-shell__layout {
  display: flex;
}

.project-shell__masthead {
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 14px 0 22px;
  border-bottom: 1px solid var(--prts-border);
}

.project-shell__identity {
  align-items: center;
  min-width: 0;
  gap: 14px;
}

.project-shell__avatar {
  flex: 0 0 auto;
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
  color: var(--prts-accent);
  font-family: var(--font-mono);
  font-size: 16px;
}

.project-shell__title {
  display: grid;
  min-width: 0;
  gap: 4px;
}

.project-shell__layout {
  align-items: flex-start;
  gap: 28px;
  padding-top: 24px;
}

.project-shell__nav {
  position: sticky;
  top: calc(var(--prts-nav-h) + 18px);
  display: grid;
  width: 196px;
  flex: 0 0 196px;
  border: 1px solid var(--prts-border);
  background: var(--prts-panel);
}

.project-shell__nav-item {
  display: grid;
  grid-template-columns: 22px 1fr auto;
  align-items: center;
  gap: 8px;
  min-height: 44px;
  padding: 0 12px;
  border: 0;
  border-bottom: 1px solid var(--prts-border-soft);
  background: transparent;
  color: var(--prts-text-dim);
  font: 13px var(--font-sans);
  text-align: left;
  cursor: pointer;
}

.project-shell__nav-item:last-child {
  border-bottom: 0;
}

.project-shell__nav-item:hover:not(:disabled),
.project-shell__nav-item--active {
  background: var(--prts-panel-2);
  color: var(--prts-text-strong);
}

.project-shell__nav-item--active {
  box-shadow: inset 2px 0 var(--prts-accent);
}

.project-shell__nav-item--pending {
  cursor: default;
  opacity: 0.56;
}

.project-shell__soon {
  font: 9px var(--font-mono);
  color: var(--prts-text-faint);
  text-transform: uppercase;
}

.project-shell__content {
  min-width: 0;
  flex: 1 1 auto;
}

@media (max-width: 820px) {
  .project-shell__masthead {
    align-items: flex-start;
  }

  .project-shell__layout {
    display: grid;
  }

  .project-shell__nav {
    position: static;
    display: flex;
    width: 100%;
    overflow-x: auto;
  }

  .project-shell__nav-item {
    grid-template-columns: 18px auto auto;
    min-width: max-content;
    border-right: 1px solid var(--prts-border-soft);
    border-bottom: 0;
  }
}
</style>

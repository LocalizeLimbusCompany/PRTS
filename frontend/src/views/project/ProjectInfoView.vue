<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import MarkdownView from '@/components/MarkdownView.vue'
import ProjectAvatar from '@/components/project/ProjectAvatar.vue'
import ProjectProgress from '@/components/project/ProjectProgress.vue'
import { langLabel } from '@/lib/langs'
import { useProjectWorkspace } from '@/lib/projectWorkspace'
import { apiErrorMessage, projectsApi, type MemberDto, type ProjectJoinInfoDto } from '@/api'
import { shouldConnectProjectRealtime, useRealtime } from '@/composables/useRealtime'
import { useAuthStore } from '@/stores/auth'

const { detail, projectId } = useProjectWorkspace()
const auth = useAuthStore()
const $q = useQuasar()
const { locale, t } = useI18n()
const router = useRouter()
const project = computed(() => detail.value?.project)
const localizedLangLabel = (code: string) => langLabel(code, locale.value)
const members = ref<MemberDto[]>([])
const joinInfo = ref<ProjectJoinInfoDto | null>(null)
const joinLoading = ref(false)
const joinPassword = ref('')
const joinAnswer = ref('')
const joinMessage = ref('')
const isCurrentMember = computed(() =>
  members.value.some((member) => member.user_id === auth.user?.id),
)
const { presences } = useRealtime(
  () => projectId.value,
  {},
  () =>
    shouldConnectProjectRealtime(auth.isAuthed, detail.value?.capabilities.collaborate === true),
)
const onlineMembers = computed(() => {
  const ids = new Set(presences.value.map((presence) => presence.user_id))
  return [...ids].map((id) => {
    const member = members.value.find((candidate) => candidate.user_id === id)
    if (member) return member
    return { user_id: id, username: `#${id}`, avatar_url: null } as MemberDto
  })
})
onMounted(async () => {
  if (auth.isAuthed) members.value = await projectsApi.members(projectId.value).catch(() => [])
  if (auth.isAuthed && project.value?.visibility === 'public') {
    joinInfo.value = await projectsApi.joinInfo(projectId.value).catch(() => null)
  }
})

async function submitJoin() {
  const info = joinInfo.value
  if (!info) return
  joinLoading.value = true
  try {
    const result = await projectsApi.join(projectId.value, {
      password: info.join_policy === 'password' ? joinPassword.value : undefined,
      answer: info.join_policy === 'quiz' ? joinAnswer.value : undefined,
      message: info.join_policy === 'application' ? joinMessage.value : undefined,
    })
    joinInfo.value = await projectsApi.joinInfo(projectId.value)
    members.value = await projectsApi.members(projectId.value).catch(() => members.value)
    $q.notify({
      type: 'positive',
      message: t(result.status === 'pending' ? 'project.join.pending' : 'project.join.joined'),
    })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.join.failed')) })
  } finally {
    joinPassword.value = ''
    joinAnswer.value = ''
    joinLoading.value = false
  }
}

async function withdrawOrLeave() {
  joinLoading.value = true
  try {
    await projectsApi.withdrawOrLeave(projectId.value)
    members.value = members.value.filter((member) => member.user_id !== auth.user?.id)
    if (project.value?.visibility === 'public') {
      joinInfo.value = await projectsApi.joinInfo(projectId.value)
    } else {
      await router.replace({ name: 'projects' })
    }
    $q.notify({ type: 'positive', message: t('project.join.left') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    joinLoading.value = false
  }
}
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
              {{ localizedLangLabel(language) }}
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
            {{ localizedLangLabel(project.target_lang) }}
          </div>
        </div>
      </div>
    </div>

    <q-card
      v-if="joinInfo && !joinInfo.is_member && joinInfo.join_policy !== 'admin_only'"
      flat
      bordered
      class="project-info__join"
    >
      <q-card-section class="column q-gutter-sm">
        <div class="row items-center">
          <div>
            <div class="prts-label">{{ $t('project.join.heading') }}</div>
            <div class="prts-dim">{{ $t(`project.join.policies.${joinInfo.join_policy}`) }}</div>
          </div>
          <q-space />
          <q-btn
            v-if="joinInfo.pending_application_id"
            outline
            no-caps
            color="negative"
            icon="mdi-close-circle-outline"
            :label="$t('project.join.withdraw')"
            :loading="joinLoading"
            @click="withdrawOrLeave"
          />
        </div>
        <q-input
          v-if="!joinInfo.pending_application_id && joinInfo.join_policy === 'password'"
          v-model="joinPassword"
          outlined
          dense
          type="password"
          :label="$t('project.join.password')"
          :disable="joinLoading"
        />
        <q-input
          v-if="!joinInfo.pending_application_id && joinInfo.join_policy === 'quiz'"
          v-model="joinAnswer"
          outlined
          dense
          :label="joinInfo.quiz_question ?? $t('project.join.answer')"
          :disable="joinLoading"
        />
        <q-input
          v-if="!joinInfo.pending_application_id && joinInfo.join_policy === 'application'"
          v-model="joinMessage"
          outlined
          dense
          type="textarea"
          autogrow
          :label="$t('project.join.message')"
          :disable="joinLoading"
        />
        <div>
          <q-btn
            v-if="!joinInfo.pending_application_id"
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            icon="mdi-account-plus-outline"
            :label="
              $t(
                joinInfo.join_policy === 'application'
                  ? 'project.join.apply'
                  : 'project.join.action',
              )
            "
            :loading="joinLoading"
            @click="submitJoin"
          />
        </div>
      </q-card-section>
    </q-card>

    <q-card
      v-if="(joinInfo?.is_member || isCurrentMember) && project.owner_id !== auth.user?.id"
      flat
      bordered
      class="project-info__join"
    >
      <q-card-section class="row items-center q-gutter-sm">
        <q-icon name="mdi-account-check-outline" color="positive" size="20px" />
        <span>{{ $t('project.join.member') }}</span>
        <q-space />
        <q-btn
          outline
          no-caps
          color="negative"
          icon="mdi-logout"
          :label="$t('project.join.leave')"
          :loading="joinLoading"
          @click="withdrawOrLeave"
        />
      </q-card-section>
    </q-card>

    <q-card flat bordered class="project-info__progress">
      <q-card-section>
        <ProjectProgress
          :state-counts="detail.state_counts"
          :questioned-count="detail.questioned_count"
          :total="detail.entry_count"
        />
      </q-card-section>
    </q-card>

    <q-card v-if="detail.capabilities.collaborate" flat bordered>
      <q-card-section>
        <div class="row items-center q-gutter-sm">
          <div>
            <div class="prts-label">{{ $t('project.online.heading') }}</div>
            <div class="prts-dim">
              {{ $t('project.online.count', { count: onlineMembers.length }) }}
            </div>
          </div>
          <q-space />
          <div class="project-info__online">
            <q-avatar
              v-for="member in onlineMembers"
              :key="member.user_id"
              size="34px"
              color="primary"
              text-color="dark"
            >
              <img v-if="member.avatar_url" :src="member.avatar_url" :alt="member.username" />
              <span v-else>{{ member.username.charAt(0).toUpperCase() }}</span>
              <q-tooltip>{{ member.username }}</q-tooltip>
            </q-avatar>
            <span v-if="onlineMembers.length === 0" class="prts-dim">{{
              $t('project.online.empty')
            }}</span>
          </div>
        </div>
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
.project-info__online {
  display: flex;
  align-items: center;
}
.project-info__online :deep(.q-avatar) {
  margin-left: -7px;
  border: 2px solid var(--prts-bg);
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

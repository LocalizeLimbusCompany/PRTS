<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, projectsApi, type MemberDto } from '@/api'
import { hasProjectCapability } from '@/lib/capabilities'
import { ROLE_LABELS, roleLabel } from '@/lib/states'
import { useAuthStore } from '@/stores/auth'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId } = useProjectWorkspace()
const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()
const { t } = useI18n()

const members = ref<MemberDto[]>([])
const showAddMember = ref(false)
const newMember = ref({ username: '', role: 'translator' })
const roleOptions = ['manager', 'reviewer', 'translator']

/** Refresh the compatibility member panel. */
async function loadMembers() {
  members.value = await projectsApi.members(projectId.value)
}

async function addMember() {
  if (!newMember.value.username.trim()) return
  try {
    await projectsApi.addMember(projectId.value, {
      username: newMember.value.username.trim(),
      role: newMember.value.role,
    })
    showAddMember.value = false
    newMember.value.username = ''
    await loadMembers()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

async function removeMember(member: MemberDto) {
  try {
    await projectsApi.removeMember(projectId.value, member.user_id)
    await loadMembers()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

function confirmDelete() {
  $q.dialog({
    title: t('project.legacy.deleteTitle'),
    message: t('project.legacy.deleteWarning', { name: detail.value?.project.name }),
    cancel: true,
    ok: { label: t('project.legacy.deleteAction'), color: 'negative', noCaps: true },
  }).onOk(async () => {
    try {
      await projectsApi.remove(projectId.value)
      await router.push({ name: 'projects' })
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    }
  })
}

onMounted(() => {
  void loadMembers()
})
</script>

<template>
  <section class="legacy-controls">
    <q-banner dense class="legacy-controls__notice">
      <template #avatar><q-icon name="mdi-progress-wrench" color="warning" /></template>
      {{ $t('project.legacy.notice') }}
    </q-banner>

    <q-card flat bordered>
      <q-card-section class="row items-center">
        <div>
          <div class="prts-label">{{ $t('project.legacy.members') }}</div>
          <div class="prts-dim q-mt-xs">{{ members.length }} {{ $t('project.legacy.people') }}</div>
        </div>
        <q-space />
        <q-btn
          v-if="hasProjectCapability(detail?.capabilities, 'manage_members')"
          flat
          no-caps
          icon="mdi-account-plus-outline"
          :label="$t('project.legacy.addMember')"
          @click="showAddMember = true"
        />
      </q-card-section>
      <q-separator />
      <q-list separator>
        <q-item v-for="member in members" :key="member.user_id">
          <q-item-section avatar>
            <q-avatar square size="34px" color="primary" text-color="dark">
              <img v-if="member.avatar_url" :src="member.avatar_url" alt="" />
              <span v-else>{{ member.username.slice(0, 2).toUpperCase() }}</span>
            </q-avatar>
          </q-item-section>
          <q-item-section>
            <q-item-label>{{ member.username }}</q-item-label>
            <q-item-label caption>{{ roleLabel(member.role) }}</q-item-label>
          </q-item-section>
          <q-item-section side>
            <div class="row no-wrap">
              <q-btn
                v-if="member.user_id !== auth.user?.id"
                flat
                round
                dense
                icon="mdi-email-outline"
                :to="{ name: 'message-thread', params: { userId: member.user_id } }"
              />
              <q-btn
                v-if="hasProjectCapability(detail?.capabilities, 'manage_members')"
                flat
                round
                dense
                color="negative"
                icon="mdi-account-remove-outline"
                @click="removeMember(member)"
              />
            </div>
          </q-item-section>
        </q-item>
      </q-list>
    </q-card>

    <q-card
      v-if="hasProjectCapability(detail?.capabilities, 'delete_project')"
      flat
      bordered
      class="legacy-controls__danger"
    >
      <q-card-section class="legacy-controls__section">
        <div>
          <div class="prts-label text-negative">{{ $t('project.legacy.danger') }}</div>
          <div class="prts-dim q-mt-xs">{{ $t('project.legacy.deleteHint') }}</div>
        </div>
        <q-space />
        <q-btn
          outline
          no-caps
          color="negative"
          icon="mdi-delete-outline"
          :label="$t('project.legacy.deleteAction')"
          @click="confirmDelete"
        />
      </q-card-section>
    </q-card>

    <q-dialog v-model="showAddMember">
      <q-card style="width: 420px; max-width: 92vw">
        <q-card-section
          ><div class="prts-h2">{{ $t('project.legacy.addMember') }}</div></q-card-section
        >
        <q-card-section class="column q-gutter-md">
          <q-input
            v-model="newMember.username"
            dense
            outlined
            :label="$t('project.legacy.username')"
          />
          <q-select
            v-model="newMember.role"
            dense
            outlined
            :options="roleOptions"
            :option-label="(role) => ROLE_LABELS[role] ?? role"
            :label="$t('project.legacy.role')"
          />
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps :label="$t('project.cancel')" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :label="$t('project.save')"
            @click="addMember"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </section>
</template>

<style scoped>
.legacy-controls,
.legacy-controls__section {
  display: grid;
  gap: 14px;
}

.legacy-controls__notice {
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
}

.legacy-controls__section {
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: center;
}

.legacy-controls__danger {
  border-color: color-mix(in srgb, var(--prts-danger) 48%, var(--prts-border));
}

@media (max-width: 760px) {
  .legacy-controls__section {
    grid-template-columns: 1fr;
  }
}
</style>

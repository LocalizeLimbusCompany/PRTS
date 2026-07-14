<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useQuasar } from 'quasar'

import { apiErrorMessage, projectsApi, type DeleteChallengeDto } from '@/api'

const props = defineProps<{ modelValue: boolean; projectId: number; slug: string }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean]; scheduled: [] }>()
const $q = useQuasar()

const step = ref(1)
const consequencesConfirmed = ref(false)
const slugInput = ref('')
const challenge = ref<DeleteChallengeDto | null>(null)
const answerInput = ref('')
const loading = ref(false)
const slugMatches = computed(() => slugInput.value === props.slug)

watch(
  () => props.modelValue,
  (open) => {
    if (!open) return
    step.value = 1
    consequencesConfirmed.value = false
    slugInput.value = ''
    challenge.value = null
    answerInput.value = ''
  },
)

function continueToSlug() {
  if (consequencesConfirmed.value) step.value = 2
}

async function requestChallenge() {
  if (!consequencesConfirmed.value || step.value !== 2 || !slugMatches.value) return
  loading.value = true
  try {
    challenge.value = await projectsApi.deleteChallenge(props.projectId)
    step.value = 3
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    loading.value = false
  }
}

async function scheduleDeletion() {
  if (!challenge.value || !answerInput.value.trim()) return
  loading.value = true
  try {
    await projectsApi.scheduleDeletion(props.projectId, {
      challenge_id: challenge.value.challenge_id,
      answer: Number(answerInput.value),
    })
    emit('update:modelValue', false)
    emit('scheduled')
  } catch (error) {
    challenge.value = null
    step.value = 2
    answerInput.value = ''
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <q-dialog
    :model-value="modelValue"
    persistent
    @update:model-value="emit('update:modelValue', $event)"
  >
    <q-card class="delete-dialog">
      <q-card-section>
        <div class="prts-label text-negative">{{ $t('project.deletion.label') }}</div>
        <div class="prts-h2 q-mt-xs">{{ $t('project.deletion.heading') }}</div>
      </q-card-section>
      <q-card-section v-if="step === 1" class="column q-gutter-md">
        <q-banner class="bg-negative text-white">
          {{ $t('project.deletion.consequences') }}
        </q-banner>
        <p>{{ $t('project.deletion.waitingPeriod') }}</p>
        <p>{{ $t('project.deletion.readonly') }}</p>
        <q-checkbox v-model="consequencesConfirmed" :label="$t('project.deletion.understand')" />
      </q-card-section>
      <q-card-section v-else-if="step === 2" class="column q-gutter-md">
        <p>{{ $t('project.deletion.slugPrompt', { slug }) }}</p>
        <q-input v-model="slugInput" outlined autofocus :label="$t('project.deletion.slug')" />
      </q-card-section>
      <q-card-section v-else class="column q-gutter-md">
        <p>{{ challenge?.prompt }}</p>
        <q-input
          v-model="answerInput"
          outlined
          inputmode="numeric"
          :label="$t('project.deletion.answer')"
        />
      </q-card-section>
      <q-card-actions align="right">
        <q-btn
          flat
          no-caps
          :label="$t('project.cancel')"
          @click="emit('update:modelValue', false)"
        />
        <q-btn
          v-if="step === 1"
          unelevated
          no-caps
          color="negative"
          :disable="!consequencesConfirmed"
          :label="$t('project.deletion.continue')"
          @click="continueToSlug"
        />
        <q-btn
          v-else-if="step === 2"
          unelevated
          no-caps
          color="negative"
          :disable="!slugMatches"
          :loading="loading"
          :label="$t('project.deletion.getChallenge')"
          @click="requestChallenge"
        />
        <q-btn
          v-else
          unelevated
          no-caps
          color="negative"
          :disable="!answerInput.trim()"
          :loading="loading"
          :label="$t('project.deletion.schedule')"
          @click="scheduleDeletion"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.delete-dialog {
  width: 560px;
  max-width: 94vw;
}
</style>

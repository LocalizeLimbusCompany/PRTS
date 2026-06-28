<script setup lang="ts">
import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import { useAppStore } from '@/stores/app'

const { t } = useI18n()
const store = useAppStore()
const { online, version, loading } = storeToRefs(store)

onMounted(() => {
  store.checkBackend()
})
</script>

<template>
  <q-page class="flex flex-center column q-pa-lg">
    <div class="text-h3 text-weight-bold q-mb-sm">{{ t('home.title') }}</div>
    <div class="text-subtitle1 text-grey-7 q-mb-xl">{{ t('home.subtitle') }}</div>

    <q-card flat bordered class="q-pa-md" style="min-width: 320px">
      <q-card-section class="row items-center justify-between">
        <span class="text-weight-medium">{{ t('home.backendStatus') }}</span>
        <q-spinner v-if="loading" size="sm" />
        <q-badge v-else-if="online === true" color="positive">{{ t('home.online') }}</q-badge>
        <q-badge v-else-if="online === false" color="negative">{{ t('home.offline') }}</q-badge>
        <span v-else class="text-grey">{{ t('home.checking') }}</span>
      </q-card-section>

      <q-separator />

      <q-card-section v-if="version" class="row items-center justify-between">
        <span class="text-weight-medium">{{ t('home.version') }}</span>
        <code>{{ version }}</code>
      </q-card-section>

      <q-card-actions align="right">
        <q-btn
          color="primary"
          unelevated
          no-caps
          :loading="loading"
          :label="t('home.refresh')"
          @click="store.checkBackend()"
        />
      </q-card-actions>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { locale, t } = useI18n()

function toggleLocale() {
  locale.value = locale.value === 'zh-CN' ? 'en' : 'zh-CN'
}
</script>

<template>
  <q-layout view="hHh lpR fFf">
    <q-header elevated class="bg-primary text-white">
      <q-toolbar>
        <q-toolbar-title class="row items-center">
          <span class="text-weight-bold">PRTS</span>
          <span class="text-caption q-ml-sm">{{ t('app.tagline') }}</span>
        </q-toolbar-title>
        <q-btn
          flat
          dense
          no-caps
          icon="translate"
          :label="t('app.switchLang')"
          @click="toggleLocale"
        />
      </q-toolbar>
    </q-header>

    <q-page-container>
      <router-view />
    </q-page-container>
  </q-layout>
</template>

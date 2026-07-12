<script setup lang="ts">
import ProjectShell from '@/views/project/ProjectShell.vue'

defineProps<{ id: number }>()
</script>

<template>
  <ProjectShell :id="id" />
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

const props = defineProps<{ modelValue: boolean; file: File | null }>()
const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  cropped: [blob: Blob]
}>()
const $q = useQuasar()
const { t } = useI18n()

const canvas = ref<HTMLCanvasElement | null>(null)
const zoom = ref(1)
const offsetX = ref(0)
const offsetY = ref(0)
const rendering = ref(false)
let source: HTMLImageElement | null = null
let sourceUrl: string | null = null

function cleanupSource() {
  if (sourceUrl) URL.revokeObjectURL(sourceUrl)
  sourceUrl = null
  source = null
}

function draw() {
  const target = canvas.value
  if (!target || !source) return
  const context = target.getContext('2d')
  if (!context) return
  const cropSize = Math.min(source.naturalWidth, source.naturalHeight) / zoom.value
  const sourceX = ((source.naturalWidth - cropSize) * (offsetX.value + 100)) / 200
  const sourceY = ((source.naturalHeight - cropSize) * (offsetY.value + 100)) / 200
  context.clearRect(0, 0, target.width, target.height)
  context.imageSmoothingEnabled = true
  context.imageSmoothingQuality = 'high'
  context.drawImage(source, sourceX, sourceY, cropSize, cropSize, 0, 0, target.width, target.height)
}

async function loadSource(file: File | null) {
  cleanupSource()
  if (!file) return
  sourceUrl = URL.createObjectURL(file)
  const image = new Image()
  image.decoding = 'async'
  image.src = sourceUrl
  try {
    await image.decode()
  } catch {
    cleanupSource()
    emit('update:modelValue', false)
    $q.notify({ type: 'negative', message: t('project.avatar.invalidImage') })
    return
  }
  source = image
  zoom.value = 1
  offsetX.value = 0
  offsetY.value = 0
  await nextTick()
  draw()
}

function close() {
  emit('update:modelValue', false)
}

async function confirm() {
  if (!canvas.value) return
  rendering.value = true
  try {
    const blob = await new Promise<Blob>((resolve, reject) => {
      canvas.value?.toBlob(
        (value) => (value ? resolve(value) : reject(new Error('webp_encode_failed'))),
        'image/webp',
        0.9,
      )
    })
    if (blob.type !== 'image/webp') throw new Error('webp_not_supported')
    emit('cropped', blob)
    close()
  } catch {
    $q.notify({ type: 'negative', message: t('project.avatar.encodeFailed') })
  } finally {
    rendering.value = false
  }
}

watch(
  () => [props.modelValue, props.file] as const,
  ([open, file]) => {
    if (open) void loadSource(file)
  },
)
watch([zoom, offsetX, offsetY], draw)
onBeforeUnmount(cleanupSource)
</script>

<template>
  <q-dialog :model-value="modelValue" persistent @update:model-value="emit('update:modelValue', $event)">
    <q-card class="avatar-crop">
      <q-card-section class="avatar-crop__head">
        <div>
          <div class="prts-label">{{ $t('project.avatar.cropLabel') }}</div>
          <h3>{{ $t('project.avatar.cropHeading') }}</h3>
        </div>
        <q-btn flat round dense icon="mdi-close" :aria-label="$t('project.cancel')" @click="close" />
      </q-card-section>

      <q-card-section class="avatar-crop__workspace">
        <div class="avatar-crop__preview">
          <canvas ref="canvas" width="256" height="256" />
          <span class="avatar-crop__corner avatar-crop__corner--one" />
          <span class="avatar-crop__corner avatar-crop__corner--two" />
        </div>
        <div class="avatar-crop__controls">
          <label>
            <span>{{ $t('project.avatar.zoom') }}</span>
            <q-slider v-model="zoom" :min="1" :max="3" :step="0.01" color="primary" />
          </label>
          <label>
            <span>{{ $t('project.avatar.horizontal') }}</span>
            <q-slider v-model="offsetX" :min="-100" :max="100" color="primary" />
          </label>
          <label>
            <span>{{ $t('project.avatar.vertical') }}</span>
            <q-slider v-model="offsetY" :min="-100" :max="100" color="primary" />
          </label>
          <p class="prts-dim">{{ $t('project.avatar.outputHint') }}</p>
        </div>
      </q-card-section>

      <q-card-actions align="right">
        <q-btn flat no-caps :label="$t('project.cancel')" @click="close" />
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-crop-square"
          :label="$t('project.avatar.useCrop')"
          :loading="rendering"
          @click="confirm"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.avatar-crop {
  width: min(760px, 94vw);
  border-top: 2px solid var(--prts-accent);
  border-radius: 2px;
}

.avatar-crop__head,
.avatar-crop__workspace {
  display: flex;
  justify-content: space-between;
  gap: 28px;
}

.avatar-crop__head h3 {
  margin: 4px 0 0;
  font: 500 20px var(--font-display);
}

.avatar-crop__preview {
  position: relative;
  width: min(320px, 42vw);
  aspect-ratio: 1;
  flex: 0 0 auto;
  padding: 12px;
  border: 1px solid var(--prts-border);
  background:
    linear-gradient(45deg, var(--prts-panel-2) 25%, transparent 25%) 0 0 / 16px 16px,
    linear-gradient(-45deg, var(--prts-panel-2) 25%, transparent 25%) 0 0 / 16px 16px,
    var(--prts-panel);
}

.avatar-crop__preview canvas {
  display: block;
  width: 100%;
  height: 100%;
}

.avatar-crop__corner {
  position: absolute;
  width: 22px;
  height: 22px;
  border-color: var(--prts-accent);
}

.avatar-crop__corner--one {
  top: 7px;
  left: 7px;
  border-top: 2px solid;
  border-left: 2px solid;
}

.avatar-crop__corner--two {
  right: 7px;
  bottom: 7px;
  border-right: 2px solid;
  border-bottom: 2px solid;
}

.avatar-crop__controls {
  display: grid;
  flex: 1;
  align-content: center;
  gap: 16px;
}

.avatar-crop__controls label {
  display: grid;
  gap: 4px;
  color: var(--prts-text-strong);
  font-size: 12px;
}

.avatar-crop__controls p {
  margin: 0;
  padding-top: 12px;
  border-top: 1px solid var(--prts-border-soft);
}

@media (max-width: 640px) {
  .avatar-crop__workspace {
    display: grid;
  }

  .avatar-crop__preview {
    width: min(320px, 82vw);
    margin: 0 auto;
  }
}
</style>

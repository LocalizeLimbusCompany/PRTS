<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import {
  apiErrorMessage,
  entriesApi,
  projectsApi,
  type FileDto,
  type FolderDto,
  type MemberDto,
  type ProjectDetailDto,
} from '@/api'
import { ROLE_LABELS, STATE_ORDER, roleLabel, stateLabel } from '@/lib/states'
import { useAuthStore } from '@/stores/auth'

const props = defineProps<{ id: number }>()
const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()

const detail = ref<ProjectDetailDto | null>(null)
const folders = ref<FolderDto[]>([])
const files = ref<FileDto[]>([])
const members = ref<MemberDto[]>([])
const loading = ref(true)

const myRole = computed(() => {
  if (auth.isAdmin) return 'owner'
  return members.value.find((m) => m.user_id === auth.user?.id)?.role ?? null
})
const canManage = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const canUpload = computed(() => canManage.value)
const canDelete = computed(() => myRole.value === 'owner' || auth.isAdmin)

const progress = computed(() => {
  if (!detail.value || detail.value.entry_count === 0) return 0
  const untrans = detail.value.state_counts['untranslated'] ?? 0
  return (detail.value.entry_count - untrans) / detail.value.entry_count
})

interface TreeNode {
  key: string
  label: string
  icon: string
  isFile: boolean
  fileId?: number
  count?: number
  children?: TreeNode[]
}

const treeNodes = computed<TreeNode[]>(() => {
  const folderNodes = new Map<number, TreeNode>()
  for (const f of folders.value) {
    folderNodes.set(f.id, {
      key: `d${f.id}`,
      label: f.name,
      icon: 'folder',
      isFile: false,
      children: [],
    })
  }
  const roots: TreeNode[] = []
  for (const f of folders.value) {
    const node = folderNodes.get(f.id)!
    if (f.parent_id && folderNodes.has(f.parent_id))
      folderNodes.get(f.parent_id)!.children!.push(node)
    else roots.push(node)
  }
  for (const file of files.value) {
    const node: TreeNode = {
      key: `f${file.id}`,
      label: file.name,
      icon: 'description',
      isFile: true,
      fileId: file.id,
      count: file.entry_count,
    }
    if (file.folder_id && folderNodes.has(file.folder_id))
      folderNodes.get(file.folder_id)!.children!.push(node)
    else roots.push(node)
  }
  return roots
})

async function loadAll() {
  loading.value = true
  try {
    const [d, tree, mem] = await Promise.all([
      projectsApi.get(props.id),
      projectsApi.tree(props.id),
      projectsApi.members(props.id),
    ])
    detail.value = d
    folders.value = tree.folders
    files.value = tree.files
    members.value = mem
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '加载失败') })
  } finally {
    loading.value = false
  }
}
onMounted(loadAll)

function openFile(node: TreeNode) {
  if (node.isFile && node.fileId) {
    router.push({ name: 'editor', params: { id: props.id }, query: { file: node.fileId } })
  }
}

async function doExport() {
  try {
    const blob = await projectsApi.exportProject(props.id)
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${detail.value?.project.slug ?? 'project'}.zip`
    a.click()
    URL.revokeObjectURL(url)
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '导出失败') })
  }
}

function confirmDelete() {
  $q.dialog({
    title: '删除项目',
    message: `确认删除「${detail.value?.project.name}」？此操作不可恢复。`,
    cancel: true,
    ok: { label: '删除', color: 'negative', noCaps: true },
  }).onOk(async () => {
    try {
      await projectsApi.remove(props.id)
      $q.notify({ type: 'positive', message: '已删除' })
      router.push('/projects')
    } catch (e) {
      $q.notify({ type: 'negative', message: apiErrorMessage(e, '删除失败') })
    }
  })
}

/* —— 上传 —— */
const showUpload = ref(false)
const uploadPath = ref('')
const uploadJson = ref('')
const pickedFile = ref<File | null>(null)
const uploading = ref(false)

async function onPickFile(file: File | null) {
  if (!file) return
  if (!uploadPath.value) uploadPath.value = file.name
  uploadJson.value = await file.text()
}
async function doUpload() {
  let entries: unknown
  try {
    entries = JSON.parse(uploadJson.value)
  } catch {
    $q.notify({ type: 'negative', message: 'JSON 解析失败' })
    return
  }
  if (!Array.isArray(entries)) {
    $q.notify({ type: 'negative', message: '内容应为词条数组 [...]' })
    return
  }
  uploading.value = true
  try {
    const res = await entriesApi.upload(props.id, {
      path: uploadPath.value.trim(),
      entries: entries as Array<Record<string, unknown>>,
    })
    $q.notify({
      type: 'positive',
      message: `上传完成 · 新增 ${res.created} · 更新 ${res.updated} · 未变 ${res.unchanged}`,
    })
    showUpload.value = false
    uploadJson.value = ''
    uploadPath.value = ''
    pickedFile.value = null
    await loadAll()
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '上传失败') })
  } finally {
    uploading.value = false
  }
}

/* —— 成员 —— */
const showAddMember = ref(false)
const newMember = ref({ username: '', role: 'translator' })
const roleOptions = ['owner', 'manager', 'reviewer', 'translator']

async function addMember() {
  if (!newMember.value.username.trim()) return
  try {
    await projectsApi.addMember(props.id, {
      username: newMember.value.username.trim(),
      role: newMember.value.role,
    })
    showAddMember.value = false
    newMember.value.username = ''
    members.value = await projectsApi.members(props.id)
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '添加失败') })
  }
}
async function removeMember(m: MemberDto) {
  try {
    await projectsApi.removeMember(props.id, m.user_id)
    members.value = members.value.filter((x) => x.user_id !== m.user_id)
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '移除失败') })
  }
}
</script>

<template>
  <q-page class="prts-container">
    <q-inner-loading :showing="loading" />

    <template v-if="detail">
      <!-- header -->
      <div class="row items-start q-mb-md">
        <div class="col">
          <div class="prts-label">// PROJECT</div>
          <div class="row items-center q-gutter-sm">
            <h1 class="prts-h1">{{ detail.project.name }}</h1>
            <q-badge
              v-if="detail.project.visibility === 'private'"
              outline
              color="grey"
              label="私有"
            />
          </div>
          <div class="prts-mono prts-dim q-mt-xs" style="font-size: 12px">
            {{ detail.project.slug }} ·
            <span>{{ detail.project.source_langs.join(' · ') || '—' }}</span>
            <q-icon name="east" size="13px" />
            <span class="text-accent">{{ detail.project.target_lang }}</span>
          </div>
          <div v-if="detail.project.description" class="prts-dim q-mt-sm" style="max-width: 720px">
            {{ detail.project.description }}
          </div>
        </div>
        <div class="col-auto row q-gutter-sm">
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            icon="edit_note"
            label="翻译编辑器"
            :to="{ name: 'editor', params: { id: props.id } }"
          />
          <q-btn outline no-caps color="primary" icon="download" label="导出" @click="doExport" />
          <q-btn
            v-if="canUpload"
            outline
            no-caps
            icon="upload"
            label="上传"
            @click="showUpload = true"
          />
          <q-btn
            v-if="canDelete"
            flat
            round
            dense
            icon="delete"
            color="negative"
            @click="confirmDelete"
          >
            <q-tooltip>删除项目</q-tooltip>
          </q-btn>
        </div>
      </div>

      <!-- stats -->
      <q-card flat bordered class="q-pa-md q-mb-lg">
        <div class="row items-center q-mb-sm">
          <div class="prts-label">进度</div>
          <q-space />
          <div class="prts-mono prts-dim" style="font-size: 12px">
            {{ Math.round(progress * 100) }}% · {{ detail.entry_count }} 词条
          </div>
        </div>
        <q-linear-progress
          :value="progress"
          color="primary"
          track-color="dark"
          rounded
          size="8px"
        />
        <div class="row q-gutter-md q-mt-md">
          <div v-for="s in STATE_ORDER" :key="s" class="row items-center" style="gap: 7px">
            <span class="state-dot" :class="'state-' + s" />
            <span class="prts-dim" style="font-size: 12px">{{ stateLabel(s) }}</span>
            <span class="prts-mono" style="font-size: 13px">{{ detail.state_counts[s] ?? 0 }}</span>
          </div>
        </div>
      </q-card>

      <div class="row q-col-gutter-lg">
        <!-- file tree -->
        <div class="col-12 col-md-7">
          <div class="prts-label q-mb-sm">文件</div>
          <q-card flat bordered class="q-pa-sm">
            <div v-if="treeNodes.length === 0" class="prts-empty" style="padding: 36px">
              暂无文件 · 上传 JSON 以创建
            </div>
            <q-tree
              v-else
              :nodes="treeNodes"
              node-key="key"
              default-expand-all
              no-connectors
              selected-color="primary"
            >
              <template #default-header="prop">
                <div class="row items-center full-width tree-row" @click="openFile(prop.node)">
                  <q-icon
                    :name="prop.node.icon"
                    size="16px"
                    class="q-mr-sm"
                    :color="prop.node.isFile ? 'primary' : 'grey'"
                  />
                  <span :class="{ 'prts-mono': prop.node.isFile }">{{ prop.node.label }}</span>
                  <q-space />
                  <span v-if="prop.node.isFile" class="prts-mono prts-dim" style="font-size: 11px">
                    {{ prop.node.count }}
                  </span>
                </div>
              </template>
            </q-tree>
          </q-card>
        </div>

        <!-- members -->
        <div class="col-12 col-md-5">
          <div class="row items-center q-mb-sm">
            <div class="prts-label">成员</div>
            <q-space />
            <q-btn
              v-if="canManage"
              flat
              dense
              no-caps
              size="sm"
              icon="person_add"
              label="添加"
              @click="showAddMember = true"
            />
          </div>
          <q-card flat bordered>
            <q-list separator>
              <q-item v-for="m in members" :key="m.user_id">
                <q-item-section avatar>
                  <q-avatar size="30px" color="primary" text-color="dark">
                    <img v-if="m.avatar_url" :src="m.avatar_url" alt="" />
                    <span v-else>{{ m.username.slice(0, 2).toUpperCase() }}</span>
                  </q-avatar>
                </q-item-section>
                <q-item-section>
                  <q-item-label>{{ m.username }}</q-item-label>
                  <q-item-label caption>{{ roleLabel(m.role) }}</q-item-label>
                </q-item-section>
                <q-item-section v-if="canManage" side>
                  <q-btn flat round dense size="sm" icon="close" @click="removeMember(m)" />
                </q-item-section>
              </q-item>
            </q-list>
          </q-card>
        </div>
      </div>
    </template>

    <!-- upload dialog -->
    <q-dialog v-model="showUpload">
      <q-card style="width: 560px; max-width: 94vw">
        <q-card-section><div class="prts-h2">上传词条</div></q-card-section>
        <q-card-section class="column q-gutter-md">
          <q-input
            v-model="uploadPath"
            outlined
            dense
            label="文件路径（如 dialog/ch1.json，自动建文件夹）"
            :disable="uploading"
          />
          <q-file
            v-model="pickedFile"
            outlined
            dense
            accept=".json"
            label="选择 JSON 文件"
            :disable="uploading"
            @update:model-value="onPickFile"
          >
            <template #prepend><q-icon name="attach_file" /></template>
          </q-file>
          <q-input
            v-model="uploadJson"
            outlined
            dense
            type="textarea"
            input-class="prts-mono"
            :input-style="{ minHeight: '160px', fontSize: '12px' }"
            label="词条 JSON 数组 [{ key, original:{lang:text}, context? }]"
            :disable="uploading"
          />
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps label="取消" :disable="uploading" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            label="上传"
            :loading="uploading"
            @click="doUpload"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>

    <!-- add member dialog -->
    <q-dialog v-model="showAddMember">
      <q-card style="width: 400px; max-width: 92vw">
        <q-card-section><div class="prts-h2">添加成员</div></q-card-section>
        <q-card-section class="column q-gutter-md">
          <q-input v-model="newMember.username" outlined dense label="用户名" autofocus />
          <q-select
            v-model="newMember.role"
            outlined
            dense
            :options="roleOptions"
            :option-label="(r) => ROLE_LABELS[r] ?? r"
            label="角色"
          />
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps label="取消" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            label="添加"
            @click="addMember"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>

<style scoped>
.tree-row {
  cursor: pointer;
  padding: 2px 0;
}
</style>

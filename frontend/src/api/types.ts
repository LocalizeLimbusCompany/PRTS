// 后端 DTO 的 TypeScript 映射（P0–P2）。

export interface UserDto {
  id: number
  username: string
  email: string | null
  avatar_url: string | null
  description: string
  translation_langs: string[]
  cp: number
  platform_role: string | null
  created_at: string
}

export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
  user: UserDto
}

export interface ProjectDto {
  id: number
  slug: string
  name: string
  description: string
  visibility: string
  source_langs: string[]
  target_lang: string
  owner_id: number
  created_at: string
  updated_at: string
}

export interface ProjectDetailDto {
  project: ProjectDto
  state_counts: Record<string, number>
  entry_count: number
}

export interface MemberDto {
  user_id: number
  username: string
  avatar_url: string | null
  role: string
  created_at: string
}

export interface FolderDto {
  id: number
  parent_id: number | null
  name: string
  path: string
}

export interface FileDto {
  id: number
  folder_id: number | null
  name: string
  path: string
  entry_count: number
}

export interface ProjectTree {
  folders: FolderDto[]
  files: FileDto[]
}

export interface EntryDto {
  id: number
  file_id: number
  key: string
  original: Record<string, string>
  context: string
  translation: string
  state: string
  locked: boolean
  hidden: boolean
  version: number
  updated_at: string
}

export interface EntryVersionDto {
  version: number
  kind: string
  translation: string | null
  state: string | null
  original: Record<string, string> | null
  editor_id: number | null
  created_at: string
}

export interface UploadResult {
  file_id: number
  created: number
  updated: number
  unchanged: number
}

export interface ApiKeyDto {
  id: number
  name: string
  prefix: string
  created_at: string
  last_used_at: string | null
}

export interface CreatedApiKey {
  id: number
  name: string
  prefix: string
  created_at: string
  key: string
}

export interface ExternalAccountDto {
  provider: string
  external_id: string
  created_at: string
}

/** 词条工作流状态。 */
export const ENTRY_STATES = [
  'untranslated',
  'translated',
  'questioned',
  'checked',
  'reviewed',
] as const
export type EntryState = (typeof ENTRY_STATES)[number]

/** 混合搜索结果（EntryDto + RRF 相关度分值）。 */
export interface SearchHitDto extends EntryDto {
  relevance: number
}

/** 搜索 / 向量化配置（可写部分）。 */
export interface SearchConfigDto {
  embedding_enabled: boolean
  embedding_model: string
  embedding_base_url: string
  embedding_batch: number
  tm_enabled: boolean
  tm_min_similarity: number
  tm_top_n: number
}

/** 搜索 / 向量化设置（GET 响应：含密钥是否已配置的只读标志）。 */
export interface SearchSettingsDto extends SearchConfigDto {
  embedding_key_present: boolean
}

/** TM 翻译建议（来自其他项目的复用）。 */
export interface SuggestionDto {
  entry_id: number
  project_id: number
  project_name: string
  source_text: string
  translation: string
  state: string
  similarity: number
}

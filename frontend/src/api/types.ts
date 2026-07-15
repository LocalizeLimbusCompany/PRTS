// 后端 DTO 的 TypeScript 映射（P0–P2）。

export interface UserDto {
  id: number
  username: string
  email: string | null
  avatar_url: string | null
  description: string
  translation_langs: string[]
  cp_tenths: number
  platform_role: string | null
  platform_capabilities: PlatformCapabilities
  password_change_required: boolean
  created_at: string
}

export interface PlatformCapabilities {
  access_admin: boolean
  grant_platform_roles: boolean
  manage_users: boolean
  create_project: boolean
  manage_pos: boolean
}

export type AdminUserSort = 'username_asc' | 'username_desc' | 'created_at_asc' | 'created_at_desc'

export interface AdminUserCapabilities {
  can_change_role: boolean
}

export interface AdminUserDto {
  id: number
  username: string
  platform_role: string | null
  password_change_required: boolean
  created_at: string
  capabilities: AdminUserCapabilities
}

export interface AdminUserListCapabilities {
  create_user: boolean
  assignable_roles: string[]
}

export interface AdminUserListResponse {
  items: AdminUserDto[]
  next_after: string | null
  capabilities: AdminUserListCapabilities
}

export interface AdminUserListParams {
  q?: string
  role?: string
  sort?: AdminUserSort
  after?: string
  limit?: number
}

export interface CreateAdminUserRequest {
  username: string
  initial_password: string
  role: string
}

export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
  user: UserDto
}

/** 登录与注册页可安全使用的公开认证能力。 */
export interface AuthConfigDto {
  password_login_enabled: boolean
  password_registration_enabled: boolean
  oauth_providers: string[]
}

export type LeaderboardPeriod = 'all' | 'month' | 'week'

export interface LeaderboardEntryDto {
  rank: number
  user_id: number
  username: string
  avatar_url: string | null
  cp_tenths: number
}

export interface LeaderboardResponse {
  period: LeaderboardPeriod
  period_start: string | null
  period_end: string | null
  items: LeaderboardEntryDto[]
}

export interface ProjectDto {
  id: number
  slug: string
  name: string
  description: string
  visibility: string
  source_langs: string[]
  primary_source_lang: string | null
  target_lang: string
  language_repair_state: string
  primary_source_changed_at: string | null
  primary_source_cooldown_until: string | null
  lexical_state: string
  lexical_job_id: number | null
  embedding_state: string
  embedding_job_id: number | null
  embedding_degraded_reason: string | null
  avatar_url: string | null
  avatar_updated_at: string | null
  deletion_scheduled_at: string | null
  deletion_job_id: number | null
  owner_id: number
  created_at: string
  updated_at: string
}

export interface ProjectCapabilities {
  view_project: boolean
  manage_project: boolean
  manage_members: boolean
  member_assignable_roles: string[]
  upload_files: boolean
  view_file_history: boolean
  rollback_file_history: boolean
  manage_tasks: boolean
  manage_terms: boolean
  download: boolean
  edit_entry: boolean
  review_entry: boolean
  lock_entry: boolean
  hide_entry: boolean
  edit_locked_entry: boolean
  force_save_presence: boolean
  collaborate: boolean
  resolve_languages: boolean
  change_primary_source: boolean
  delete_project: boolean
}

export interface ProjectDetailDto {
  project: ProjectDto
  state_counts: Record<string, number>
  entry_count: number
  capabilities: ProjectCapabilities
}

export interface MemberDto {
  user_id: number
  username: string
  avatar_url: string | null
  role: string
  created_at: string
  capabilities: MemberCapabilities
}

export interface MemberCapabilities {
  assignable_roles: string[]
  can_change_role: boolean
  can_remove: boolean
}

export interface DeleteChallengeDto {
  challenge_id: string
  prompt: string
  expires_in: number
}

export interface DeletionStatusDto {
  project_id: number
  slug: string
  deletion_scheduled_at: string
  deletion_job_id: number
}

export interface FolderDto {
  id: number
  parent_id: number | null
  name: string
  path: string
  created_at: string
}

export interface FileDto {
  id: number
  folder_id: number | null
  name: string
  path: string
  entry_count: number
  state_counts: Record<string, number>
  created_at: string
  updated_at: string
}

export interface ProjectTree {
  folders: FolderDto[]
  files: FileDto[]
}

export interface FileOperationDto {
  change_set_id: string
  purge_after: string | null
}

export interface FileChangeItemDto {
  id: number
  entity_type: 'folder' | 'file' | 'entry'
  entity_id: number | null
  operation: string
  before: Record<string, unknown> | null
  after: Record<string, unknown> | null
  ordinal: number
  created_at: string
}

export interface FileChangeSetDto {
  id: string
  file_id: number | null
  folder_id: number | null
  actor_id: number | null
  operation: string
  path_snapshot: string
  metadata: Record<string, unknown>
  created_at: string
  items: FileChangeItemDto[]
}

export interface FileHistoryPage {
  items: FileChangeSetDto[]
  next_after: string | null
}

export interface EntryDto {
  id: number
  file_id: number
  key: string
  original: Record<string, string>
  translation: string
  state: EntryState
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

/** 服务端下发的上传运行时限制；上传客户端不得复制这些默认值。 */
export interface UploadConfigDto {
  max_files_per_batch: number
  max_bytes_per_file: number
  max_bytes_per_batch: number
  client_concurrency: number
  upload_batch_expiry_hours: number
}

export interface JobDto {
  id: number
  kind: string
  project_id: number | null
  state: string
  stage: string
  progress_current: number
  progress_total: number | null
  attempts: number
  max_attempts: number
  next_retry_at: string | null
  last_error_code: string | null
  manual_retry_allowed: boolean
  created_at: string
  started_at: string | null
  finished_at: string | null
  updated_at: string
}

export interface LanguageIssueDto {
  id: number
  entity_type: string
  entity_id: string
  issue_kind: string
  raw_tag: string | null
  canonical_tag: string | null
  metadata: Record<string, unknown>
  current_values: string[]
}

export interface ProjectLanguageResolutionDto {
  project_id: number
  source_langs: string[]
  primary_source_lang: string | null
  target_lang: string
  state: string
  issues: LanguageIssueDto[]
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

export type SearchOperator = 'contains' | 'not_contains' | 'starts_with' | 'ends_with' | 'equals'

export interface SearchCondition {
  field: string
  operator: SearchOperator
  value: string
}

export type SearchScope =
  | { type: 'all' }
  | { type: 'path'; path: string }
  | { type: 'file'; file_id: number }
  | { type: 'current_file'; file_id: number }
  | { type: 'current_task'; task_id: number }

export interface StructuredSearchRequest {
  query?: string
  conditions: SearchCondition[]
  scope: SearchScope
  states: EntryState[]
  include_hidden: boolean
  vector: boolean
  after?: string
  limit?: number
}

/** 结构化搜索结果（EntryDto + RRF 相关度分值）。 */
export interface SearchHitDto extends EntryDto {
  rrf_score: number
}

export interface StructuredSearchResponse {
  items: SearchHitDto[]
  next_after: string | null
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

/** 通知（收件人维度；`type` 为通知种类，如 "poke"）。 */
export interface NotificationDto {
  id: number
  type: string
  payload: Record<string, unknown>
  read_at: string | null
  created_at: string
}

/** 私信（一条消息）。 */
export interface MessageDto {
  id: number
  sender_id: number
  recipient_id: number
  content: string
  read_at: string | null
  created_at: string
}

/** 会话摘要（会话列表项：对话方资料 + 最后一条 + 我方未读数）。 */
export interface ThreadDto {
  other_user_id: number
  username: string
  avatar_url: string | null
  last_content: string
  last_sender_id: number
  last_created_at: string
  unread: number
}

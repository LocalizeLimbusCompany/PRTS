import { http } from './http'
import { streamAiExplanation, type AiStreamCallbacks } from './aiStream'
import type {
  ApiKeyDto,
  AiExplanationDto,
  AiSettingsDto,
  AiSettingsWriteRequest,
  ApiKeyScope,
  AiSourcePreference,
  AiUiLocale,
  AdminUserDto,
  AdminUserListParams,
  AdminUserListResponse,
  AuthConfigDto,
  CreatedApiKey,
  DeleteChallengeDto,
  DeletionStatusDto,
  EntryCommentDto,
  EntryCommentPageDto,
  EntryDto,
  EntryVersionDto,
  ExternalAccountDto,
  FileHistoryPage,
  FileOperationDto,
  MemberDto,
  JobDto,
  LeaderboardPeriod,
  LeaderboardResponse,
  ProjectLanguageResolutionDto,
  MessageDto,
  NotificationDto,
  ThreadDto,
  ProjectDetailDto,
  ProjectDto,
  ProjectHistoryPageDto,
  ProjectJoinInfoDto,
  ProjectJoinResultDto,
  ProjectJoinSettingsDto,
  ProjectJoinPolicy,
  ProjectJoinDefaultRole,
  ProjectHistoryVisibility,
  JoinApplicationPageDto,
  MemberCandidateDto,
  ProjectTree,
  SearchConfigDto,
  StructuredSearchRequest,
  StructuredSearchResponse,
  SearchSettingsDto,
  SuggestionDto,
  TokenResponse,
  CreateAdminUserRequest,
  UserDto,
} from './types'

export * from './types'
export * from './aiStream'
export * from './tasks'
export * from './terms'
export { http, apiErrorMessage } from './http'
export { tasksApi } from './tasks'
export { posApi, termsApi } from './terms'

/** 认证。 */
export const authApi = {
  config() {
    return http.get<AuthConfigDto>('/meta/auth-config').then((r) => r.data)
  },
  register(body: { username: string; email?: string; password: string }) {
    return http.post<TokenResponse>('/auth/register', body).then((r) => r.data)
  },
  login(body: { username: string; password: string }) {
    return http.post<TokenResponse>('/auth/login', body).then((r) => r.data)
  },
  logout(refresh_token: string) {
    return http.post('/auth/logout', { refresh_token })
  },
  oauthStart(provider: string) {
    return http.get<{ authorize_url: string }>(`/auth/oauth/${provider}/start`).then((r) => r.data)
  },
}

/** 持久化后台任务。 */
export const jobsApi = {
  get(id: number) {
    return http.get<JobDto>(`/jobs/${id}`).then((response) => response.data)
  },
  retry(id: number) {
    return http.post<JobDto>(`/jobs/${id}/retry`).then((response) => response.data)
  },
}

/** 用户自助。 */
export const usersApi = {
  me() {
    return http.get<UserDto>('/me').then((r) => r.data)
  },
  updateMe(body: {
    description?: string
    avatar_url?: string | null
    translation_langs?: string[]
    entry_diff_mode?: string
    preview_translation_diff?: boolean
    ai_source_preference?: 'auto' | 'personal' | 'project'
  }) {
    return http.put<UserDto>('/me', body).then((r) => r.data)
  },
  changePassword(body: { current_password: string; new_password: string }) {
    return http.put('/me/password', body)
  },
  accounts() {
    return http.get<ExternalAccountDto[]>('/me/accounts').then((r) => r.data)
  },
  listApiKeys() {
    return http.get<ApiKeyDto[]>('/me/api-keys').then((r) => r.data)
  },
  listApiKeyScopes() {
    return http.get<ApiKeyScope[]>('/me/api-key-scopes').then((r) => r.data)
  },
  createApiKey(name: string, scopes: ApiKeyScope[]) {
    return http.post<CreatedApiKey>('/me/api-keys', { name, scopes }).then((r) => r.data)
  },
  updateApiKey(id: number, name: string, scopes: ApiKeyScope[]) {
    return http.put<ApiKeyDto>(`/me/api-keys/${id}`, { name, scopes }).then((r) => r.data)
  },
  revokeApiKey(id: number) {
    return http.delete(`/me/api-keys/${id}`)
  },
  /** 公开用户资料（不含 email）：私信会话页展示对话方头名/头像。 */
  getUser(id: number) {
    return http.get<UserDto>(`/users/${id}`).then((r) => r.data)
  },
}

/** 用户/项目 OpenAI-compatible 设置与按需原文解释。 */
export const aiApi = {
  getPersonalSettings() {
    return http.get<AiSettingsDto>('/me/ai-settings').then((response) => response.data)
  },
  putPersonalSettings(body: AiSettingsWriteRequest) {
    return http.put<AiSettingsDto>('/me/ai-settings', body).then((response) => response.data)
  },
  deletePersonalSettings() {
    return http.delete('/me/ai-settings')
  },
  getProjectSettings(projectId: number) {
    return http
      .get<AiSettingsDto>(`/projects/${projectId}/ai-settings`)
      .then((response) => response.data)
  },
  putProjectSettings(projectId: number, body: AiSettingsWriteRequest) {
    return http
      .put<AiSettingsDto>(`/projects/${projectId}/ai-settings`, body)
      .then((response) => response.data)
  },
  deleteProjectSettings(projectId: number) {
    return http.delete(`/projects/${projectId}/ai-settings`)
  },
  explainEntry(
    projectId: number,
    entryId: number,
    uiLocale: AiUiLocale,
    source?: AiSourcePreference,
  ) {
    return http
      .post<AiExplanationDto>(`/projects/${projectId}/entries/${entryId}/ai-explanation`, {
        source,
        ui_locale: uiLocale,
      })
      .then((response) => response.data)
  },
  streamExplainEntry(
    projectId: number,
    entryId: number,
    uiLocale: AiUiLocale,
    source: AiSourcePreference | undefined,
    callbacks: AiStreamCallbacks,
    signal?: AbortSignal,
  ) {
    return streamAiExplanation(projectId, entryId, uiLocale, source, callbacks, signal)
  },
}

/** 公开贡献排行榜。 */
export const leaderboardsApi = {
  platform(period: LeaderboardPeriod) {
    return http
      .get<LeaderboardResponse>('/leaderboards/platform', { params: { period } })
      .then((response) => response.data)
  },
  project(projectId: number) {
    return http
      .get<LeaderboardResponse>(`/projects/${projectId}/leaderboard`)
      .then((response) => response.data)
  },
}

/** 项目与成员、文件树。 */
export const projectsApi = {
  list(params: { mine?: boolean; page?: number; per_page?: number } = {}) {
    return http.get<ProjectDto[]>('/projects', { params }).then((r) => r.data)
  },
  create(body: {
    name: string
    slug?: string
    description?: string
    visibility?: string
    source_langs: string[]
    primary_source_lang?: string
    target_lang: string
    join_policy: ProjectJoinPolicy
    join_default_role?: ProjectJoinDefaultRole
  }) {
    return http.post<ProjectDto>('/projects', body).then((r) => r.data)
  },
  get(id: number) {
    return http.get<ProjectDetailDto>(`/projects/${id}`).then((r) => r.data)
  },
  update(id: number, body: Record<string, unknown>) {
    return http.put<ProjectDto>(`/projects/${id}`, body).then((r) => r.data)
  },
  changePrimarySource(id: number, body: { source_langs: string[]; primary_source_lang: string }) {
    return http.put<ProjectDto>(`/projects/${id}/primary-source`, body).then((r) => r.data)
  },
  languageResolution(id: number) {
    return http
      .get<ProjectLanguageResolutionDto>(`/projects/${id}/language-resolution`)
      .then((r) => r.data)
  },
  resolveLanguages(
    id: number,
    body: {
      source_langs: string[]
      primary_source_lang: string
      target_lang: string
      issues: Array<{
        issue_id: number
        canonical_tag?: string
        selected_value?: string
      }>
    },
  ) {
    return http.post(`/projects/${id}/language-resolution/resolve`, body)
  },
  deleteChallenge(id: number) {
    return http.post<DeleteChallengeDto>(`/projects/${id}/delete-challenge`).then((r) => r.data)
  },
  scheduleDeletion(id: number, body: { challenge_id: string; answer: number }) {
    return http.delete<DeletionStatusDto>(`/projects/${id}`, { data: body }).then((r) => r.data)
  },
  deletionStatus(id: number) {
    return http.get<DeletionStatusDto>(`/projects/${id}/deletion`).then((r) => r.data)
  },
  cancelDeletion(id: number) {
    return http.post(`/projects/${id}/deletion/cancel`)
  },
  members(id: number) {
    return http.get<MemberDto[]>(`/projects/${id}/members`).then((r) => r.data)
  },
  addMember(id: number, body: { username: string; role: string }) {
    return http.post(`/projects/${id}/members`, body)
  },
  removeMember(id: number, userId: number) {
    return http.delete(`/projects/${id}/members/${userId}`)
  },
  joinInfo(id: number) {
    return http.get<ProjectJoinInfoDto>(`/projects/${id}/join`).then((r) => r.data)
  },
  join(id: number, body: { password?: string; answer?: string; message?: string }) {
    return http.post<ProjectJoinResultDto>(`/projects/${id}/join`, body).then((r) => r.data)
  },
  withdrawOrLeave(id: number) {
    return http.delete(`/projects/${id}/join`)
  },
  joinSettings(id: number) {
    return http.get<ProjectJoinSettingsDto>(`/projects/${id}/join-settings`).then((r) => r.data)
  },
  updateJoinSettings(
    id: number,
    body: {
      join_policy: ProjectJoinPolicy
      join_default_role: ProjectJoinDefaultRole
      history_visibility: ProjectHistoryVisibility
      password?: string
      quiz_question?: string
      quiz_answer?: string
    },
  ) {
    return http
      .put<ProjectJoinSettingsDto>(`/projects/${id}/join-settings`, body)
      .then((r) => r.data)
  },
  joinApplications(id: number, params: { after?: number; limit?: number } = {}) {
    return http
      .get<JoinApplicationPageDto>(`/projects/${id}/join-applications`, { params })
      .then((r) => r.data)
  },
  decideJoinApplication(
    id: number,
    applicationId: number,
    body: { approved: boolean; role?: string },
  ) {
    return http.post(`/projects/${id}/join-applications/${applicationId}/decision`, body)
  },
  memberCandidates(id: number, q: string) {
    return http
      .get<MemberCandidateDto[]>(`/projects/${id}/member-candidates`, { params: { q } })
      .then((r) => r.data)
  },
  history(id: number, params: { after?: string; limit?: number } = {}) {
    return http
      .get<ProjectHistoryPageDto>(`/projects/${id}/history`, { params })
      .then((r) => r.data)
  },
  tree(id: number) {
    return http.get<ProjectTree>(`/projects/${id}/tree`).then((r) => r.data)
  },
  createFolder(id: number, body: { parent_id: number | null; name: string }) {
    return http.post(`/projects/${id}/folders`, body).then((r) => r.data)
  },
  moveFile(id: number, fileId: number, body: { folder_id: number | null; name: string }) {
    return http.patch<FileOperationDto>(`/projects/${id}/files/${fileId}`, body).then((r) => r.data)
  },
  moveFolder(id: number, folderId: number, body: { parent_id: number | null; name: string }) {
    return http
      .patch<FileOperationDto>(`/projects/${id}/folders/${folderId}`, body)
      .then((r) => r.data)
  },
  deleteFile(id: number, fileId: number) {
    return http.delete(`/projects/${id}/files/${fileId}`)
  },
  deleteFolder(id: number, folderId: number) {
    return http.delete(`/projects/${id}/folders/${folderId}`)
  },
  restoreFile(id: number, fileId: number, deletionChangeSetId: string) {
    return http
      .post<FileOperationDto>(`/projects/${id}/files/${fileId}/restore`, {
        deletion_change_set_id: deletionChangeSetId,
      })
      .then((r) => r.data)
  },
  restoreFolder(id: number, folderId: number, deletionChangeSetId: string) {
    return http
      .post<FileOperationDto>(`/projects/${id}/folders/${folderId}/restore`, {
        deletion_change_set_id: deletionChangeSetId,
      })
      .then((r) => r.data)
  },
  async exportProject(id: number): Promise<Blob> {
    const r = await http.get(`/projects/${id}/export`, { responseType: 'blob' })
    return r.data as Blob
  },
  async avatar(id: number): Promise<Blob> {
    const response = await http.get(`/projects/${id}/avatar`, { responseType: 'blob' })
    return response.data as Blob
  },
  uploadAvatar(id: number, avatar: Blob) {
    return http.post(`/projects/${id}/avatar`, avatar, {
      headers: { 'Content-Type': 'image/webp' },
    })
  },
  deleteAvatar(id: number) {
    return http.delete(`/projects/${id}/avatar`)
  },
}

/** File change-set history and server-materialized rollback. */
export const fileHistoryApi = {
  list(
    projectId: number,
    params: { after?: string; file_id?: number; folder_id?: number; limit?: number } = {},
  ) {
    return http
      .get<FileHistoryPage>(`/projects/${projectId}/file-history`, { params })
      .then((response) => response.data)
  },
  rollbackFile(projectId: number, fileId: number, changeSetId: string) {
    return http
      .post<FileOperationDto>(
        `/projects/${projectId}/files/${fileId}/history/${changeSetId}/rollback`,
      )
      .then((response) => response.data)
  },
  rollbackFolder(projectId: number, folderId: number, changeSetId: string) {
    return http
      .post<FileOperationDto>(
        `/projects/${projectId}/folders/${folderId}/history/${changeSetId}/rollback`,
      )
      .then((response) => response.data)
  },
}

/** 上传与词条。 */
export const entriesApi = {
  list(
    id: number,
    params: {
      file_id?: number
      task_id?: number
      state?: string
      questioned?: boolean
      q?: string
      after?: number
      limit?: number
      include_hidden?: boolean
    } = {},
  ) {
    return http.get<EntryDto[]>(`/projects/${id}/entries`, { params }).then((r) => r.data)
  },
  get(id: number, entryId: number) {
    return http.get<EntryDto>(`/projects/${id}/entries/${entryId}`).then((r) => r.data)
  },
  count(
    id: number,
    params: {
      file_id?: number
      task_id?: number
      state?: string
      questioned?: boolean
      include_hidden?: boolean
    } = {},
  ) {
    return http
      .get<{ total_items: number }>(`/projects/${id}/entries/count`, { params })
      .then((r) => r.data)
  },
  update(
    id: number,
    entryId: number,
    body: {
      translation: string
      state: string
      version: number
      force_presence?: boolean
      questioned?: boolean
      question_reason?: string
    },
  ) {
    return http.put<EntryDto>(`/projects/${id}/entries/${entryId}`, body).then((r) => r.data)
  },
  setFlags(id: number, entryId: number, body: { locked?: boolean; hidden?: boolean }) {
    return http
      .patch<EntryDto>(`/projects/${id}/entries/${entryId}/flags`, body)
      .then((r) => r.data)
  },
  history(id: number, entryId: number) {
    return http
      .get<EntryVersionDto[]>(`/projects/${id}/entries/${entryId}/history`)
      .then((r) => r.data)
  },
}

export const entryCommentsApi = {
  list(id: number, entryId: number, params: { after?: number; limit?: number } = {}) {
    return http
      .get<EntryCommentPageDto>(`/projects/${id}/entries/${entryId}/comments`, { params })
      .then((r) => r.data)
  },
  create(id: number, entryId: number, content: string) {
    return http
      .post<EntryCommentDto>(`/projects/${id}/entries/${entryId}/comments`, { content })
      .then((r) => r.data)
  },
  update(id: number, entryId: number, commentId: number, content: string) {
    return http
      .put<EntryCommentDto>(`/projects/${id}/entries/${entryId}/comments/${commentId}`, {
        content,
      })
      .then((r) => r.data)
  },
  remove(id: number, entryId: number, commentId: number) {
    return http.delete(`/projects/${id}/entries/${entryId}/comments/${commentId}`)
  },
}

/** 结构化项目搜索（POST + 签名键集 cursor）。 */
export const searchApi = {
  search(id: number, body: StructuredSearchRequest) {
    return http
      .post<StructuredSearchResponse>(`/projects/${id}/search`, body)
      .then((response) => response.data)
  },
}

/** 搜索 / 向量化管理员配置。 */
export const adminSearchApi = {
  get() {
    return http.get<SearchSettingsDto>('/admin/settings/search').then((r) => r.data)
  },
  put(cfg: SearchConfigDto) {
    return http.put<SearchSettingsDto>('/admin/settings/search', cfg).then((r) => r.data)
  },
}

/** TM 翻译建议。 */
export const suggestionsApi = {
  forEntry(projectId: number, entryId: number) {
    return http
      .get<SuggestionDto[]>(`/projects/${projectId}/entries/${entryId}/suggestions`)
      .then((r) => r.data)
  },
}

/** 平台管理。 */
export const adminApi = {
  listUsers(params: AdminUserListParams = {}) {
    return http
      .get<AdminUserListResponse>('/admin/users', { params })
      .then((response) => response.data)
  },
  createUser(body: CreateAdminUserRequest) {
    return http.post<AdminUserDto>('/admin/users', body).then((response) => response.data)
  },
  getSettings() {
    return http.get<Record<string, unknown>>('/admin/settings').then((r) => r.data)
  },
  updateSettings(settings: Record<string, unknown>) {
    return http.put('/admin/settings', { settings })
  },
  grantRole(userId: number, role: string | null) {
    return http.post(`/admin/users/${userId}/role`, { role })
  },
}

/** 通知（键集分页 + 未读数 + 已读标记）。 */
export const notificationsApi = {
  list(before?: number, limit?: number) {
    return http
      .get<NotificationDto[]>('/notifications', { params: { before, limit } })
      .then((r) => r.data)
  },
  unreadCount() {
    return http.get<{ count: number }>('/notifications/unread_count').then((r) => r.data)
  },
  markRead(ids?: number[]) {
    return http.post('/notifications/read', { ids })
  },
}

/** 编辑器「戳一下」：向同项目成员发送即时提示。 */
export const pokeApi = {
  send(projectId: number, to_user_id: number, text: string) {
    return http.post(`/projects/${projectId}/poke`, { to_user_id, text })
  },
}

/** 私信（会话列表 + 会话消息 + 发送 + 已读 + 未读数）。 */
export const messagesApi = {
  /** 我的会话列表（每个对话方最后一条 + 我方未读数）。 */
  threads() {
    return http.get<ThreadDto[]>('/messages').then((r) => r.data)
  },
  /** 与某用户的会话消息（键集分页：before 游标 + limit）。 */
  conversation(userId: number, before?: number, limit?: number) {
    return http
      .get<MessageDto[]>(`/messages/${userId}`, { params: { before, limit } })
      .then((r) => r.data)
  },
  /** 发送一条私信（须与收件人共享 ≥1 项目）。 */
  send(to_user_id: number, content: string) {
    return http.post<{ id: number }>('/messages', { to_user_id, content }).then((r) => r.data)
  },
  /** 将与某用户的会话标记为已读。 */
  markRead(userId: number) {
    return http.post(`/messages/${userId}/read`)
  },
  /** 我的未读私信总数（顶栏 ✉️ 红点）。 */
  unreadCount() {
    return http.get<{ count: number }>('/messages/unread_count').then((r) => r.data)
  },
}

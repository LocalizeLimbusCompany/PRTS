import { http } from './http'
import type {
  ApiKeyDto,
  CreatedApiKey,
  EntryDto,
  EntryVersionDto,
  ExternalAccountDto,
  MemberDto,
  ProjectDetailDto,
  ProjectDto,
  ProjectTree,
  SearchConfigDto,
  SearchHitDto,
  SearchSettingsDto,
  SuggestionDto,
  TokenResponse,
  UploadResult,
  UserDto,
} from './types'

export * from './types'
export { http, apiErrorMessage } from './http'

/** 认证。 */
export const authApi = {
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

/** 用户自助。 */
export const usersApi = {
  me() {
    return http.get<UserDto>('/me').then((r) => r.data)
  },
  updateMe(body: {
    description?: string
    avatar_url?: string | null
    translation_langs?: string[]
  }) {
    return http.put<UserDto>('/me', body).then((r) => r.data)
  },
  accounts() {
    return http.get<ExternalAccountDto[]>('/me/accounts').then((r) => r.data)
  },
  listApiKeys() {
    return http.get<ApiKeyDto[]>('/me/api-keys').then((r) => r.data)
  },
  createApiKey(name: string) {
    return http.post<CreatedApiKey>('/me/api-keys', { name }).then((r) => r.data)
  },
  revokeApiKey(id: number) {
    return http.delete(`/me/api-keys/${id}`)
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
    target_lang: string
  }) {
    return http.post<ProjectDto>('/projects', body).then((r) => r.data)
  },
  get(id: number) {
    return http.get<ProjectDetailDto>(`/projects/${id}`).then((r) => r.data)
  },
  update(id: number, body: Record<string, unknown>) {
    return http.put<ProjectDto>(`/projects/${id}`, body).then((r) => r.data)
  },
  remove(id: number) {
    return http.delete(`/projects/${id}`)
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
  tree(id: number) {
    return http.get<ProjectTree>(`/projects/${id}/tree`).then((r) => r.data)
  },
  deleteFile(id: number, fileId: number) {
    return http.delete(`/projects/${id}/files/${fileId}`)
  },
  deleteFolder(id: number, folderId: number) {
    return http.delete(`/projects/${id}/folders/${folderId}`)
  },
  async exportProject(id: number): Promise<Blob> {
    const r = await http.get(`/projects/${id}/export`, { responseType: 'blob' })
    return r.data as Blob
  },
}

/** 上传与词条。 */
export const entriesApi = {
  upload(id: number, body: { path: string; entries: Array<Record<string, unknown>> }) {
    return http.post<UploadResult>(`/projects/${id}/upload`, body).then((r) => r.data)
  },
  list(
    id: number,
    params: {
      file_id?: number
      state?: string
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
  update(
    id: number,
    entryId: number,
    body: { translation: string; state: string; version: number },
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

/** 混合全文搜索（FTS + trgm + RRF）。 */
export const searchApi = {
  search(
    id: number,
    params: {
      q?: string
      file_id?: number
      state?: string
      sort?: string
      offset?: number
      limit?: number
      include_hidden?: boolean
    } = {},
  ) {
    return http.get<SearchHitDto[]>(`/projects/${id}/search`, { params }).then((r) => r.data)
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

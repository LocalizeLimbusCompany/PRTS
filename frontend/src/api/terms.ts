import { http } from './http'
import type { TerminologyDocumentFormat } from '@/lib/terminology'

export type TermScope = 'current' | 'archived' | 'mixed' | 'deleted'

export interface TermDto {
  id: number
  project_id: number
  source_lang: string
  source_text: string
  translation: string
  notes: string
  pos_id: number | null
  pos_name_zh_cn: string | null
  pos_name_en: string | null
  archived: boolean
  archived_at: string | null
  version: number
  deleted: boolean
  deleted_at: string | null
  created_by: number | null
  updated_by: number | null
  created_at: string
  updated_at: string
}

export interface TermPageDto {
  items: TermDto[]
  next_after: number | null
}

export interface TermVersionDto {
  version: number
  kind: string
  source_lang: string
  source_text: string
  translation: string
  notes: string
  pos_id: number | null
  archived: boolean
  deleted: boolean
  editor_id: number | null
  editor_name: string
  editor_avatar_url: string | null
  created_at: string
}

export interface TermVersionPageDto {
  items: TermVersionDto[]
  next_after: number | null
  can_restore: boolean
}

export interface TermWriteRequest {
  source_lang: string
  source_text: string
  translation: string
  notes: string
  pos_id: number | null
  archived: boolean
}

export interface PosDto {
  id: number
  name_zh_cn: string | null
  name_en: string | null
  display_name: string
  sort_order: number
  created_at: string
  updated_at: string
}

export interface PosWriteRequest {
  name_zh_cn: string | null
  name_en: string | null
  sort_order: number
}

export interface ImportWarningDto {
  row: number
  code: string
}

export interface TermPreviewRowDto {
  row: number
  source_lang: string
  source_text: string
  translation: string
  pos: string | null
  notes: string
  archived: boolean
  action: 'created' | 'updated'
}

export interface PosPreviewRowDto {
  row: number
  name_zh_cn: string | null
  name_en: string | null
  sort_order: number
  action: 'created' | 'updated'
}

export interface ImportPreviewDto<Row> {
  token: string
  digest: string
  expires_in_seconds: number
  created: number
  updated: number
  rows: Row[]
  warnings: ImportWarningDto[]
}

export interface ImportConfirmDto {
  created: number
  updated: number
  warnings: ImportWarningDto[]
}

export const termsApi = {
  /** 只匹配服务端当前 primary source 的 active terms；正文放 JSON body。 */
  match(projectId: number, sourceText: string, limit = 20) {
    return http
      .post<TermDto[]>(`/projects/${projectId}/terms/match`, {
        source_text: sourceText,
        limit,
      })
      .then((response) => response.data)
  },
  list(
    projectId: number,
    params: { scope: TermScope; after?: number; limit?: number },
  ): Promise<TermPageDto> {
    return http
      .get<TermPageDto>(`/projects/${projectId}/terms`, { params })
      .then((response) => response.data)
  },
  create(projectId: number, body: TermWriteRequest) {
    return http
      .post<TermDto>(`/projects/${projectId}/terms`, body)
      .then((response) => response.data)
  },
  update(projectId: number, termId: number, body: TermWriteRequest) {
    return http
      .put<TermDto>(`/projects/${projectId}/terms/${termId}`, body)
      .then((response) => response.data)
  },
  remove(projectId: number, termId: number) {
    return http.delete(`/projects/${projectId}/terms/${termId}`)
  },
  versions(projectId: number, termId: number, params: { after?: number; limit?: number } = {}) {
    return http
      .get<TermVersionPageDto>(`/projects/${projectId}/terms/${termId}/versions`, { params })
      .then((response) => response.data)
  },
  restoreVersion(projectId: number, termId: number, version: number) {
    return http
      .post<TermDto>(`/projects/${projectId}/terms/${termId}/versions/${version}/restore`)
      .then((response) => response.data)
  },
  previewImport(projectId: number, format: TerminologyDocumentFormat, content: string) {
    return http
      .post<ImportPreviewDto<TermPreviewRowDto>>(`/projects/${projectId}/terms/imports/preview`, {
        format,
        content,
      })
      .then((response) => response.data)
  },
  confirmImport(projectId: number, token: string, digest: string) {
    return http
      .post<ImportConfirmDto>(`/projects/${projectId}/terms/imports/${token}/confirm`, { digest })
      .then((response) => response.data)
  },
  async export(projectId: number, format: TerminologyDocumentFormat): Promise<Blob> {
    const response = await http.get(`/projects/${projectId}/terms/export`, {
      params: { format },
      responseType: 'blob',
    })
    return response.data as Blob
  },
}

export const posApi = {
  list() {
    return http.get<PosDto[]>('/pos').then((response) => response.data)
  },
  create(body: PosWriteRequest) {
    return http.post<PosDto>('/admin/pos', body).then((response) => response.data)
  },
  update(id: number, body: PosWriteRequest) {
    return http.put<PosDto>(`/admin/pos/${id}`, body).then((response) => response.data)
  },
  remove(id: number) {
    return http.delete(`/admin/pos/${id}`)
  },
  previewImport(format: TerminologyDocumentFormat, content: string) {
    return http
      .post<ImportPreviewDto<PosPreviewRowDto>>('/admin/pos/imports/preview', { format, content })
      .then((response) => response.data)
  },
  confirmImport(token: string, digest: string) {
    return http
      .post<ImportConfirmDto>(`/admin/pos/imports/${token}/confirm`, { digest })
      .then((response) => response.data)
  },
  async export(format: TerminologyDocumentFormat): Promise<Blob> {
    const response = await http.get('/admin/pos/export', {
      params: { format },
      responseType: 'blob',
    })
    return response.data as Blob
  },
}

import { http } from './http'
import type { UploadConfigDto } from './types'

export interface UploadAttemptDto {
  id: number
  attempt_number: number
  state: string
  bytes_received: number
  error_code: string | null
  started_at: string
  finished_at: string | null
}

export interface UploadBatchFileDto {
  id: number
  ordinal: number
  path: string
  declared_bytes: number
  state: string
  processing_job_id: number | null
  target_file_id: number | null
  last_error_code: string | null
  attempts: UploadAttemptDto[]
}

export interface UploadBatchDto {
  id: number
  project_id: number
  state: string
  declared_file_count: number
  declared_total_bytes: number
  expires_at: string
  files: UploadBatchFileDto[]
}

/** 上传客户端启动前读取服务端当前限制。 */
export function getUploadConfig(): Promise<UploadConfigDto> {
  return http.get<UploadConfigDto>('/meta/upload-config').then((response) => response.data)
}

export const uploadsApi = {
  createBatch(projectId: number, files: Array<{ path: string; size: number }>) {
    return http
      .post<UploadBatchDto>(`/projects/${projectId}/upload-batches`, { files })
      .then((response) => response.data)
  },
  receiveAttempt(
    projectId: number,
    batchId: number,
    fileId: number,
    attemptId: number,
    file: File,
    onProgress?: (loaded: number) => void,
  ) {
    return http.put(
      `/projects/${projectId}/upload-batches/${batchId}/files/${fileId}/attempts/${attemptId}`,
      file,
      {
        headers: { 'Content-Type': 'application/json' },
        timeout: 0,
        onUploadProgress: (event) => onProgress?.(event.loaded),
      },
    )
  },
  complete(projectId: number, batchId: number) {
    return http
      .post<UploadBatchDto>(`/projects/${projectId}/upload-batches/${batchId}/complete`)
      .then((response) => response.data)
  },
  get(projectId: number, batchId: number) {
    return http
      .get<UploadBatchDto>(`/projects/${projectId}/upload-batches/${batchId}`)
      .then((response) => response.data)
  },
  retry(projectId: number, batchId: number, fileId: number) {
    return http
      .post<UploadAttemptDto>(
        `/projects/${projectId}/upload-batches/${batchId}/files/${fileId}/retry`,
      )
      .then((response) => response.data)
  },
  cancel(projectId: number, batchId: number) {
    return http.post(`/projects/${projectId}/upload-batches/${batchId}/cancel`)
  },
}

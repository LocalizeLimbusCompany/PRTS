import { http } from './http'

export interface TaskListItemDto {
  id: number
  project_id: number
  title: string
  created_by: number | null
  denominator: number
  completed: number
  completion_ratio: number
  no_work_required: boolean
  file_count: number
  created_at: string
  updated_at: string
}

export interface TaskListPageDto {
  items: TaskListItemDto[]
  next_after: number | null
}

export interface TaskFileDto {
  id: number
  file_id_snapshot: number
  live_file_id: number | null
  name: string | null
  path: string | null
  created_at: string
}

export interface TaskDetailDto {
  id: number
  project_id: number
  title: string
  description: string
  created_by: number | null
  denominator: number
  completed: number
  completion_ratio: number
  no_work_required: boolean
  files: TaskFileDto[]
  created_at: string
  updated_at: string
}

export interface SaveTaskRequest {
  title: string
  description: string
  file_ids: number[]
}

export const tasksApi = {
  list(projectId: number, params: { after?: number; limit?: number } = {}) {
    return http
      .get<TaskListPageDto>(`/projects/${projectId}/tasks`, { params })
      .then((response) => response.data)
  },
  get(projectId: number, taskId: number) {
    return http
      .get<TaskDetailDto>(`/projects/${projectId}/tasks/${taskId}`)
      .then((response) => response.data)
  },
  create(projectId: number, body: SaveTaskRequest) {
    return http
      .post<TaskDetailDto>(`/projects/${projectId}/tasks`, body)
      .then((response) => response.data)
  },
  update(projectId: number, taskId: number, body: SaveTaskRequest) {
    return http
      .put<TaskDetailDto>(`/projects/${projectId}/tasks/${taskId}`, body)
      .then((response) => response.data)
  },
  remove(projectId: number, taskId: number) {
    return http.delete(`/projects/${projectId}/tasks/${taskId}`)
  },
}

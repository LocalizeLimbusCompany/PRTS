import axios from 'axios'

/**
 * 全局 axios 实例。
 * baseURL 为 `/api`，开发期由 Vite 代理到后端（见 vite.config.ts），
 * 生产期由 nginx 反代（见 deploy/nginx）。
 */
export const http = axios.create({
  baseURL: '/api',
  timeout: 10_000,
})

/** 后端版本信息（对应后端 GET /version）。 */
export interface VersionInfo {
  name: string
  version: string
}

/** 获取后端版本信息。 */
export async function getVersion(): Promise<VersionInfo> {
  const { data } = await http.get<VersionInfo>('/version')
  return data
}

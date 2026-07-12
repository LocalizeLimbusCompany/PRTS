import { http } from './http'
import type { UploadConfigDto } from './types'

/** 上传客户端启动前读取服务端当前限制。 */
export function getUploadConfig(): Promise<UploadConfigDto> {
  return http.get<UploadConfigDto>('/meta/upload-config').then((response) => response.data)
}

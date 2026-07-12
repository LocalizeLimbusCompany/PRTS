import { beforeEach, describe, expect, it, vi } from 'vitest'

import { getUploadConfig, uploadsApi } from '@/api/uploads'

import { useUploadBatch } from './useUploadBatch'

vi.mock('@/api/uploads', () => ({
  getUploadConfig: vi.fn(),
  uploadsApi: {
    createBatch: vi.fn(),
    receiveAttempt: vi.fn(),
    complete: vi.fn(),
    get: vi.fn(),
    cancel: vi.fn(),
  },
}))

describe('streaming upload client contract', () => {
  beforeEach(() => vi.clearAllMocks())

  it('streams the raw File without parsing JSON in the browser', async () => {
    const file = new File(['not parsed by browser'], 'raw.json', { type: 'application/json' })
    Object.defineProperty(file, 'text', {
      value: vi.fn(() => Promise.reject(new Error('browser must not parse upload JSON'))),
    })
    const batch = {
      id: 7,
      project_id: 3,
      state: 'uploading',
      declared_file_count: 1,
      declared_total_bytes: file.size,
      expires_at: new Date().toISOString(),
      files: [
        {
          id: 11,
          ordinal: 0,
          path: 'raw.json',
          declared_bytes: file.size,
          state: 'uploading',
          processing_job_id: null,
          target_file_id: null,
          last_error_code: null,
          attempts: [
            {
              id: 13,
              attempt_number: 1,
              state: 'uploading',
              bytes_received: 0,
              error_code: null,
              started_at: new Date().toISOString(),
              finished_at: null,
            },
          ],
        },
      ],
    }
    vi.mocked(getUploadConfig).mockResolvedValue({
      max_files_per_batch: 500,
      max_bytes_per_file: 100 * 1024 * 1024,
      max_bytes_per_batch: 2 * 1024 * 1024 * 1024,
      client_concurrency: 3,
      upload_batch_expiry_hours: 24,
    })
    vi.mocked(uploadsApi.createBatch).mockResolvedValue(batch)
    vi.mocked(uploadsApi.receiveAttempt).mockResolvedValue({ status: 204 } as never)
    vi.mocked(uploadsApi.complete).mockResolvedValue({ ...batch, state: 'queued' })

    const upload = useUploadBatch(() => 3)
    await upload.start([file])

    expect(file.text).not.toHaveBeenCalled()
    expect(uploadsApi.receiveAttempt).toHaveBeenCalledWith(
      3,
      7,
      11,
      13,
      file,
      expect.any(Function),
    )
  })
})

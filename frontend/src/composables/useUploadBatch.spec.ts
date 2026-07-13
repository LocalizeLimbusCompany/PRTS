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
    retry: vi.fn(),
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
          error_details: null,
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
      expect.any(AbortSignal),
    )
  })

  it('retries a failed logical file with a new byte-zero attempt', async () => {
    const file = new File(['retry bytes'], 'retry.json', { type: 'application/json' })
    const failedBatch = {
      id: 9,
      project_id: 3,
      state: 'failed',
      declared_file_count: 1,
      declared_total_bytes: file.size,
      expires_at: new Date().toISOString(),
      files: [
        {
          id: 21,
          ordinal: 0,
          path: 'retry.json',
          declared_bytes: file.size,
          state: 'failed',
          processing_job_id: 31,
          target_file_id: null,
          last_error_code: 'upload_invalid_json',
          error_details: { ordinal: 4, line: 8, column: 2 },
          attempts: [
            {
              id: 22,
              attempt_number: 1,
              state: 'failed',
              bytes_received: file.size,
              error_code: 'upload_invalid_json',
              started_at: new Date().toISOString(),
              finished_at: new Date().toISOString(),
            },
          ],
        },
      ],
    }
    vi.mocked(getUploadConfig).mockResolvedValue({
      max_files_per_batch: 500,
      max_bytes_per_file: 100 * 1024 * 1024,
      max_bytes_per_batch: 2 * 1024 * 1024 * 1024,
      client_concurrency: 2,
      upload_batch_expiry_hours: 24,
    })
    vi.mocked(uploadsApi.createBatch).mockResolvedValue(failedBatch)
    vi.mocked(uploadsApi.receiveAttempt)
      .mockRejectedValueOnce(new Error('first transfer failed'))
      .mockResolvedValueOnce({ status: 204 } as never)
    vi.mocked(uploadsApi.get).mockResolvedValue(failedBatch)
    vi.mocked(uploadsApi.retry).mockResolvedValue({
      id: 23,
      attempt_number: 2,
      state: 'uploading',
      bytes_received: 0,
      error_code: null,
      started_at: new Date().toISOString(),
      finished_at: null,
    })
    vi.mocked(uploadsApi.complete).mockResolvedValue({ ...failedBatch, state: 'queued' })

    const upload = useUploadBatch(() => 3)
    await upload.start([file])
    await upload.retry(failedBatch.files[0]!)

    expect(uploadsApi.retry).toHaveBeenCalledWith(3, 9, 21)
    expect(uploadsApi.receiveAttempt).toHaveBeenLastCalledWith(
      3,
      9,
      21,
      23,
      file,
      expect.any(Function),
      expect.any(AbortSignal),
    )
    expect(upload.queue.value[0]?.loaded).toBe(file.size)
  })

  it('submits received files while preserving failed siblings for later retry', async () => {
    const received = new File(['ok'], 'ok.json', { type: 'application/json' })
    const failed = new File(['bad'], 'bad.json', { type: 'application/json' })
    const batch = {
      id: 10,
      project_id: 3,
      state: 'uploading',
      declared_file_count: 2,
      declared_total_bytes: received.size + failed.size,
      expires_at: new Date().toISOString(),
      files: [received, failed].map((file, index) => ({
        id: 30 + index,
        ordinal: index,
        path: file.name,
        declared_bytes: file.size,
        state: 'uploading',
        processing_job_id: null,
        target_file_id: null,
        last_error_code: null,
        error_details: null,
        attempts: [
          {
            id: 40 + index,
            attempt_number: 1,
            state: 'uploading',
            bytes_received: 0,
            error_code: null,
            started_at: new Date().toISOString(),
            finished_at: null,
          },
        ],
      })),
    }
    vi.mocked(getUploadConfig).mockResolvedValue({
      max_files_per_batch: 500,
      max_bytes_per_file: 100 * 1024 * 1024,
      max_bytes_per_batch: 2 * 1024 * 1024 * 1024,
      client_concurrency: 1,
      upload_batch_expiry_hours: 24,
    })
    vi.mocked(uploadsApi.createBatch).mockResolvedValue(batch)
    vi.mocked(uploadsApi.receiveAttempt)
      .mockResolvedValueOnce({ status: 204 } as never)
      .mockRejectedValueOnce(new Error('transfer failed'))
    vi.mocked(uploadsApi.complete).mockResolvedValue({ ...batch, state: 'queued' })

    const upload = useUploadBatch(() => 3)
    await upload.start([received, failed])

    expect(uploadsApi.complete).toHaveBeenCalledWith(3, 10)
    expect(upload.queue.value.map((item) => item.state)).toEqual(['uploaded', 'failed'])
  })

  it('aborts the active transfer and does not schedule remaining files after cancellation', async () => {
    const files = ['one.json', 'two.json', 'three.json'].map(
      (name) => new File([name], name, { type: 'application/json' }),
    )
    const batch = {
      id: 11,
      project_id: 3,
      state: 'uploading',
      declared_file_count: files.length,
      declared_total_bytes: files.reduce((total, file) => total + file.size, 0),
      expires_at: new Date().toISOString(),
      files: files.map((file, index) => ({
        id: 50 + index,
        ordinal: index,
        path: file.name,
        declared_bytes: file.size,
        state: 'uploading',
        processing_job_id: null,
        target_file_id: null,
        last_error_code: null,
        error_details: null,
        attempts: [
          {
            id: 60 + index,
            attempt_number: 1,
            state: 'uploading',
            bytes_received: 0,
            error_code: null,
            started_at: new Date().toISOString(),
            finished_at: null,
          },
        ],
      })),
    }
    vi.mocked(getUploadConfig).mockResolvedValue({
      max_files_per_batch: 500,
      max_bytes_per_file: 100 * 1024 * 1024,
      max_bytes_per_batch: 2 * 1024 * 1024 * 1024,
      client_concurrency: 1,
      upload_batch_expiry_hours: 24,
    })
    vi.mocked(uploadsApi.createBatch).mockResolvedValue(batch)
    let rejectTransfer: (error: Error) => void = () => undefined
    vi.mocked(uploadsApi.receiveAttempt).mockImplementation(
      () =>
        new Promise((_, reject) => {
          rejectTransfer = reject
        }) as never,
    )
    vi.mocked(uploadsApi.cancel).mockResolvedValue({ status: 204 } as never)
    vi.mocked(uploadsApi.get).mockResolvedValue({
      ...batch,
      state: 'cancelled',
      files: batch.files.map((file) => ({ ...file, state: 'cancelled' })),
    })

    const upload = useUploadBatch(() => 3)
    const start = upload.start(files)
    await vi.waitFor(() => expect(uploadsApi.receiveAttempt).toHaveBeenCalledTimes(1))
    await upload.cancel()
    rejectTransfer(new Error('aborted'))
    await start

    expect(uploadsApi.receiveAttempt).toHaveBeenCalledTimes(1)
    expect(uploadsApi.cancel).toHaveBeenCalledWith(3, 11)
  })
})

// @vitest-environment jsdom

import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { aiApi, AiStreamRequestError, type AiExplanationDto } from '@/api'
import { useAiExplanationSessionStore } from './aiExplanationSession'

vi.mock('@/api', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/api')>()
  return {
    ...original,
    aiApi: { ...original.aiApi, streamExplainEntry: vi.fn() },
  }
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

function explanation(text: string): AiExplanationDto {
  return {
    reference_translation: text,
    tokens: [],
    grammar_notes: '',
    provider_source: 'personal',
    cached: false,
    output_tokens: 12,
    output_tokens_exact: true,
    search_status: 'disabled',
    search_used: false,
    search_provider: null,
    citations: [],
  }
}

describe('AI explanation editor sessions', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(aiApi.streamExplainEntry).mockReset()
  })

  it('keeps concurrent entry tasks and progress isolated', async () => {
    const first = deferred<AiExplanationDto>()
    const second = deferred<AiExplanationDto>()
    vi.mocked(aiApi.streamExplainEntry)
      .mockImplementationOnce((_project, _entry, _locale, _source, callbacks) => {
        callbacks.onStatus?.({ phase: 'searching' })
        return first.promise
      })
      .mockImplementationOnce((_project, _entry, _locale, _source, callbacks) => {
        callbacks.onProgress?.({ phase: 'generating', estimated_output_tokens: 7 })
        return second.promise
      })

    const store = useAiExplanationSessionStore()
    const firstRun = store.analyze(4, 10, 'zh-CN')
    const secondRun = store.analyze(4, 11, 'zh-CN')
    expect(store.getOrCreate(4, 10, 'zh-CN').phase).toBe('searching')
    expect(store.getOrCreate(4, 11, 'zh-CN').outputTokens).toBe(7)

    first.resolve(explanation('first'))
    second.resolve(explanation('second'))
    await Promise.all([firstRun, secondRun])
    expect(store.getOrCreate(4, 10, 'zh-CN').result?.reference_translation).toBe('first')
    expect(store.getOrCreate(4, 11, 'zh-CN').result?.reference_translation).toBe('second')
  })

  it('does not abort when the same session is retrieved after a tab remount', () => {
    const store = useAiExplanationSessionStore()
    const state = store.getOrCreate(4, 10, 'en')
    const controller = new AbortController()
    state.controller = controller
    expect(store.getOrCreate(4, 10, 'en').controller).toBe(controller)
    expect(controller.signal.aborted).toBe(false)
  })

  it('cancels only the selected task and clears every task when leaving a project', async () => {
    const first = deferred<AiExplanationDto>()
    const second = deferred<AiExplanationDto>()
    const signals: AbortSignal[] = []
    vi.mocked(aiApi.streamExplainEntry).mockImplementation(
      (_project, entry, _locale, _source, _callbacks, signal) => {
        signals.push(signal as AbortSignal)
        return entry === 10 ? first.promise : second.promise
      },
    )
    const store = useAiExplanationSessionStore()
    void store.analyze(4, 10, 'zh-CN')
    void store.analyze(4, 11, 'zh-CN')

    store.cancel(4, 10, 'zh-CN')
    expect(signals[0]?.aborted).toBe(true)
    expect(signals[1]?.aborted).toBe(false)
    store.clearProject(4)
    expect(signals[1]?.aborted).toBe(true)

    first.resolve(explanation('ignored'))
    second.resolve(explanation('ignored'))
    await Promise.resolve()
  })

  it('retains the prior result when a new analysis fails', async () => {
    vi.mocked(aiApi.streamExplainEntry)
      .mockResolvedValueOnce(explanation('stable result'))
      .mockRejectedValueOnce(new AiStreamRequestError('quota exhausted', 'AI_PROVIDER_ERROR'))
    const store = useAiExplanationSessionStore()
    await store.analyze(4, 10, 'zh-CN')
    await store.analyze(4, 10, 'zh-CN')
    const state = store.getOrCreate(4, 10, 'zh-CN')
    expect(state.result?.reference_translation).toBe('stable result')
    expect(state.errorCode).toBe('AI_PROVIDER_ERROR')
  })
})

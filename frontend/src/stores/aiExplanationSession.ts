import { defineStore } from 'pinia'
import { markRaw, reactive } from 'vue'

import {
  aiApi,
  AiStreamRequestError,
  type AiExplanationDto,
  type AiStreamPhase,
  type AiUiLocale,
} from '@/api'

export interface AiExplanationSessionState {
  projectId: number
  entryId: number
  uiLocale: AiUiLocale
  loading: boolean
  phase: AiStreamPhase
  elapsedSeconds: number
  outputTokens: number
  outputTokensExact: boolean
  result: AiExplanationDto | null
  errorCode: string | null
  errorMessage: string | null
  cancelled: boolean
  controller: AbortController | null
}

export function aiExplanationSessionKey(
  projectId: number,
  entryId: number,
  uiLocale: AiUiLocale,
): string {
  return `${projectId}:${entryId}:${uiLocale}`
}

/** Editor-scoped AI tasks survive context-tab and narrow-layout component unmounts. */
export const useAiExplanationSessionStore = defineStore('ai-explanation-session', () => {
  const sessions = reactive<Record<string, AiExplanationSessionState>>({})
  const elapsedTimers = new Map<string, ReturnType<typeof setInterval>>()

  function getOrCreate(
    projectId: number,
    entryId: number,
    uiLocale: AiUiLocale,
  ): AiExplanationSessionState {
    const key = aiExplanationSessionKey(projectId, entryId, uiLocale)
    const existing = sessions[key]
    if (existing) return existing
    const created: AiExplanationSessionState = {
      projectId,
      entryId,
      uiLocale,
      loading: false,
      phase: 'connecting',
      elapsedSeconds: 0,
      outputTokens: 0,
      outputTokensExact: false,
      result: null,
      errorCode: null,
      errorMessage: null,
      cancelled: false,
      controller: null,
    }
    sessions[key] = created
    return sessions[key] as AiExplanationSessionState
  }

  function stopTimer(key: string) {
    const timer = elapsedTimers.get(key)
    if (timer) clearInterval(timer)
    elapsedTimers.delete(key)
  }

  async function analyze(projectId: number, entryId: number, uiLocale: AiUiLocale): Promise<void> {
    const key = aiExplanationSessionKey(projectId, entryId, uiLocale)
    const session = getOrCreate(projectId, entryId, uiLocale)
    session.controller?.abort()
    stopTimer(key)

    const controller = markRaw(new AbortController())
    session.controller = controller
    session.loading = true
    session.phase = 'connecting'
    session.elapsedSeconds = 0
    session.outputTokens = 0
    session.outputTokensExact = false
    session.errorCode = null
    session.errorMessage = null
    session.cancelled = false
    const startedAt = Date.now()
    elapsedTimers.set(
      key,
      setInterval(() => {
        if (session.controller === controller) {
          session.elapsedSeconds = Math.floor((Date.now() - startedAt) / 1_000)
        }
      }, 1_000),
    )

    try {
      const result = await aiApi.streamExplainEntry(
        projectId,
        entryId,
        uiLocale,
        undefined,
        {
          onStatus(status) {
            if (session.controller === controller) session.phase = status.phase
          },
          onProgress(progress) {
            if (session.controller !== controller) return
            session.phase = progress.phase
            session.outputTokens = progress.estimated_output_tokens
            session.outputTokensExact = false
          },
        },
        controller.signal,
      )
      if (session.controller !== controller) return
      session.result = result
      session.outputTokens = result.output_tokens ?? session.outputTokens
      session.outputTokensExact = result.output_tokens_exact
    } catch (error) {
      if (controller.signal.aborted || session.controller !== controller) return
      session.errorCode = error instanceof AiStreamRequestError ? error.code : 'AI_REQUEST_FAILED'
      session.errorMessage = error instanceof Error ? error.message : null
    } finally {
      if (session.controller === controller) {
        session.controller = null
        session.loading = false
        stopTimer(key)
      }
    }
  }

  function cancel(projectId: number, entryId: number, uiLocale: AiUiLocale) {
    const key = aiExplanationSessionKey(projectId, entryId, uiLocale)
    const session = sessions[key]
    if (!session?.controller) return
    session.controller.abort()
    session.controller = null
    session.loading = false
    session.cancelled = true
    session.errorCode = null
    session.errorMessage = null
    stopTimer(key)
  }

  function clearProject(projectId: number) {
    for (const [key, session] of Object.entries(sessions)) {
      if (session.projectId !== projectId) continue
      session.controller?.abort()
      stopTimer(key)
      delete sessions[key]
    }
  }

  function clearAll() {
    for (const [key, session] of Object.entries(sessions)) {
      session.controller?.abort()
      stopTimer(key)
      delete sessions[key]
    }
  }

  return { sessions, getOrCreate, analyze, cancel, clearProject, clearAll }
})

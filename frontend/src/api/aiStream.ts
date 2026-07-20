import { authenticatedFetch } from './http'
import type {
  AiExplanationDto,
  AiSourcePreference,
  AiUiLocale,
  AiStreamErrorDto,
  AiStreamProgressDto,
  AiStreamStatusDto,
} from './types'

/** Callbacks expose only phase and token metadata; provider reasoning text stays server-side. */
export interface AiStreamCallbacks {
  onStatus?: (status: AiStreamStatusDto) => void
  onProgress?: (progress: AiStreamProgressDto) => void
}

export interface ParsedSseEvent {
  event: string
  data: string
}

/** Stable error shape for both HTTP setup failures and in-stream localized failures. */
export class AiStreamRequestError extends Error {
  constructor(
    message: string,
    readonly code: string,
    readonly status?: number,
  ) {
    super(message)
    this.name = 'AiStreamRequestError'
  }
}

function abortError(): DOMException {
  return new DOMException('The operation was aborted', 'AbortError')
}

/** Parse one SSE frame, including standard multi-line data fields and comment keepalives. */
function parseFrame(frame: string): ParsedSseEvent | null {
  let event = 'message'
  const data: string[] = []
  for (const line of frame.split(/\r\n|\r|\n/)) {
    if (!line || line.startsWith(':')) continue
    const separator = line.indexOf(':')
    const field = separator < 0 ? line : line.slice(0, separator)
    let value = separator < 0 ? '' : line.slice(separator + 1)
    if (value.startsWith(' ')) value = value.slice(1)
    if (field === 'event') event = value
    if (field === 'data') data.push(value)
  }
  return data.length ? { event, data: data.join('\n') } : null
}

/** Find a complete SSE event without assuming that CRLF or UTF-8 chunks share boundaries. */
function takeFrame(buffer: string): { frame: string; rest: string } | null {
  const match = /\r\n\r\n|\n\n|\r\r/.exec(buffer)
  if (!match || match.index === undefined) return null
  return {
    frame: buffer.slice(0, match.index),
    rest: buffer.slice(match.index + match[0].length),
  }
}

/** Consume a byte stream as SSE and cancel its reader immediately when the caller aborts. */
export async function parseSseStream(
  body: ReadableStream<Uint8Array>,
  onEvent: (event: ParsedSseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  const cancel = () => {
    void reader.cancel(signal?.reason)
  }
  signal?.addEventListener('abort', cancel, { once: true })

  try {
    while (true) {
      if (signal?.aborted) throw abortError()
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      let next = takeFrame(buffer)
      while (next) {
        const parsed = parseFrame(next.frame)
        if (parsed) onEvent(parsed)
        buffer = next.rest
        next = takeFrame(buffer)
      }
    }
    buffer += decoder.decode()
    const parsed = parseFrame(buffer)
    if (parsed) onEvent(parsed)
    if (signal?.aborted) throw abortError()
  } finally {
    signal?.removeEventListener('abort', cancel)
    reader.releaseLock()
  }
}

function parseJson<T>(event: ParsedSseEvent): T {
  try {
    return JSON.parse(event.data) as T
  } catch {
    throw new AiStreamRequestError('Invalid AI stream response', 'AI_RESPONSE_INVALID')
  }
}

/** Convert the server's status/progress/result/error event protocol into one final DTO. */
export async function consumeAiExplanationStream(
  body: ReadableStream<Uint8Array>,
  callbacks: AiStreamCallbacks = {},
  signal?: AbortSignal,
): Promise<AiExplanationDto> {
  let result: AiExplanationDto | null = null
  let streamError: AiStreamErrorDto | null = null

  await parseSseStream(
    body,
    (event) => {
      if (event.event === 'status') callbacks.onStatus?.(parseJson<AiStreamStatusDto>(event))
      if (event.event === 'progress') {
        callbacks.onProgress?.(parseJson<AiStreamProgressDto>(event))
      }
      if (event.event === 'result') result = parseJson<AiExplanationDto>(event)
      if (event.event === 'error') streamError = parseJson<AiStreamErrorDto>(event)
    },
    signal,
  )

  if (streamError) {
    const error = streamError as AiStreamErrorDto
    throw new AiStreamRequestError(error.message, error.code)
  }
  if (!result) throw new AiStreamRequestError('AI stream ended without a result', 'AI_STREAM_ENDED')
  return result
}

/** Start an authenticated, cancellable AI analysis stream. */
export async function streamAiExplanation(
  projectId: number,
  entryId: number,
  uiLocale: AiUiLocale,
  source: AiSourcePreference | undefined,
  callbacks: AiStreamCallbacks = {},
  signal?: AbortSignal,
): Promise<AiExplanationDto> {
  const response = await authenticatedFetch(
    `/api/projects/${projectId}/entries/${entryId}/ai-explanation/stream`,
    {
      method: 'POST',
      headers: {
        Accept: 'text/event-stream',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ source, ui_locale: uiLocale }),
      signal,
    },
  )
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as AiStreamErrorDto | null
    throw new AiStreamRequestError(
      payload?.message ?? response.statusText,
      payload?.code ?? 'AI_REQUEST_FAILED',
      response.status,
    )
  }
  if (!response.body) {
    throw new AiStreamRequestError('AI stream is unavailable', 'AI_STREAM_UNAVAILABLE')
  }
  return consumeAiExplanationStream(response.body, callbacks, signal)
}

import { describe, expect, it, vi } from 'vitest'

vi.mock('./http', () => ({ authenticatedFetch: vi.fn() }))

import {
  AiStreamRequestError,
  consumeAiExplanationStream,
  parseSseStream,
  streamAiExplanation,
} from './aiStream'
import { authenticatedFetch } from './http'

function chunkedBody(source: string, chunkSize = 1): ReadableStream<Uint8Array> {
  const bytes = new TextEncoder().encode(source)
  return new ReadableStream({
    start(controller) {
      for (let offset = 0; offset < bytes.length; offset += chunkSize) {
        controller.enqueue(bytes.slice(offset, offset + chunkSize))
      }
      controller.close()
    },
  })
}

const result = {
  overall_meaning: 'meaning',
  tokens: [],
  grammar_notes: '',
  provider_source: 'personal' as const,
  cached: false,
  output_tokens: 12,
  output_tokens_exact: true,
}

describe('AI SSE client', () => {
  it('sends the explicit UI locale in the JSON body instead of relying on headers', async () => {
    vi.mocked(authenticatedFetch).mockResolvedValue(
      new Response(chunkedBody(`event: result\ndata: ${JSON.stringify(result)}\n\n`), {
        status: 200,
      }),
    )

    await streamAiExplanation(7, 11, 'zh-CN', undefined)

    expect(authenticatedFetch).toHaveBeenCalledWith(
      '/api/projects/7/entries/11/ai-explanation/stream',
      expect.objectContaining({
        body: JSON.stringify({ source: undefined, ui_locale: 'zh-CN' }),
      }),
    )
  })

  it('parses CRLF frames split across arbitrary byte chunks', async () => {
    const events: Array<{ event: string; data: string }> = []
    await parseSseStream(
      chunkedBody(
        'event: progress\r\ndata: {"phase":"thinking",\r\ndata: "estimated_output_tokens":7}\r\n\r\n',
      ),
      (event) => events.push(event),
    )
    expect(events).toEqual([
      {
        event: 'progress',
        data: '{"phase":"thinking",\n"estimated_output_tokens":7}',
      },
    ])
  })

  it('replaces estimated progress with exact usage from the result event', async () => {
    let displayedTokens = 0
    let exact = false
    const source = [
      'event: status\ndata: {"phase":"thinking"}\n\n',
      'event: progress\ndata: {"phase":"generating","estimated_output_tokens":17}\n\n',
      `event: result\ndata: ${JSON.stringify(result)}\n\n`,
    ].join('')
    const explanation = await consumeAiExplanationStream(chunkedBody(source, 3), {
      onProgress(progress) {
        displayedTokens = progress.estimated_output_tokens
      },
    })
    displayedTokens = explanation.output_tokens ?? displayedTokens
    exact = explanation.output_tokens_exact
    expect(displayedTokens).toBe(12)
    expect(exact).toBe(true)
  })

  it('surfaces localized in-stream errors with their stable code', async () => {
    const source = 'event: error\ndata: {"code":"AI_REQUEST_TIMEOUT","message":"AI 请求超时"}\n\n'
    await expect(consumeAiExplanationStream(chunkedBody(source))).rejects.toEqual(
      expect.objectContaining<Partial<AiStreamRequestError>>({
        code: 'AI_REQUEST_TIMEOUT',
        message: 'AI 请求超时',
      }),
    )
  })

  it('cancels the response reader when the caller aborts', async () => {
    let cancelled = false
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(
          new TextEncoder().encode('event: status\ndata: {"phase":"thinking"}\n\n'),
        )
      },
      cancel() {
        cancelled = true
      },
    })
    const controller = new AbortController()
    const pending = consumeAiExplanationStream(body, {}, controller.signal)
    await Promise.resolve()
    controller.abort()
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' })
    expect(cancelled).toBe(true)
  })
})

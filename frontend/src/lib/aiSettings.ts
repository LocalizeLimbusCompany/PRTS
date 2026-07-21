import type {
  AiCustomRequestOptions,
  AiProviderPreset,
  AiReasoningEffort,
  AiSettingsDto,
  AiSettingsWriteRequest,
  AiThinkingMode,
  AiTransportMode,
  WebSearchMode,
} from '@/api/types'

const MAX_CUSTOM_OPTIONS_BYTES = 16 * 1024
const MAX_CUSTOM_OPTIONS_DEPTH = 8

export type AiSettingsValidationCode =
  | 'required'
  | 'url'
  | 'model'
  | 'apiKey'
  | 'timeout'
  | 'budget'
  | 'json'
  | 'jsonObject'
  | 'jsonTooLarge'
  | 'jsonDepth'
  | 'jsonConflict'
  | 'jsonSensitive'
  | 'searchUrl'
  | 'searchKey'
  | 'searchTimeout'
  | 'searchResults'

/** Editable string fields preserve intermediate input such as an empty numeric value. */
export interface AiSettingsFormState {
  base_url: string
  model: string
  api_key: string
  enabled: boolean
  provider_preset: AiProviderPreset
  transport_mode: AiTransportMode
  thinking_mode: AiThinkingMode
  reasoning_effort: AiReasoningEffort
  thinking_budget: string
  request_timeout_seconds: string
  custom_request_options: string
  web_search_mode: WebSearchMode
  web_search_provider: string
  web_search_endpoint: string
  web_search_api_key: string
  web_search_timeout_seconds: string
  web_search_max_results: string
  web_search_citations_enabled: boolean
}

export class AiSettingsValidationError extends Error {
  constructor(readonly code: AiSettingsValidationCode) {
    super(code)
    this.name = 'AiSettingsValidationError'
  }
}

/** Create a fresh form so decrypted credentials are never expected from a read response. */
export function createAiSettingsForm(settings: AiSettingsDto | null): AiSettingsFormState {
  return {
    base_url: settings?.base_url ?? '',
    model: settings?.model ?? '',
    api_key: '',
    enabled: settings?.configured ? settings.enabled : true,
    provider_preset: settings?.provider_preset ?? 'openai',
    transport_mode: settings?.transport_mode ?? 'auto',
    thinking_mode: settings?.thinking_mode ?? 'auto',
    reasoning_effort: settings?.reasoning_effort ?? 'medium',
    thinking_budget: settings?.thinking_budget == null ? '' : String(settings.thinking_budget),
    request_timeout_seconds: String(settings?.request_timeout_seconds ?? 180),
    custom_request_options: JSON.stringify(settings?.custom_request_options ?? {}, null, 2),
    web_search_mode: settings?.web_search_mode ?? 'disabled',
    web_search_provider: settings?.web_search_provider ?? 'tavily',
    web_search_endpoint: settings?.web_search_endpoint ?? '',
    web_search_api_key: '',
    web_search_timeout_seconds: String(settings?.web_search_timeout_seconds ?? 10),
    web_search_max_results: String(settings?.web_search_max_results ?? 5),
    web_search_citations_enabled: settings?.web_search_citations_enabled ?? true,
  }
}

function jsonDepth(value: unknown): number {
  if (Array.isArray(value)) return 1 + Math.max(0, ...value.map(jsonDepth))
  if (value !== null && typeof value === 'object') {
    return 1 + Math.max(0, ...Object.values(value).map(jsonDepth))
  }
  return 0
}

function containsSensitiveKey(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(containsSensitiveKey)
  if (value === null || typeof value !== 'object') return false
  return Object.entries(value).some(([rawKey, child]) => {
    const key = rawKey.toLowerCase().replace(/[^a-z0-9]/g, '')
    return (
      key === 'token' ||
      key.endsWith('apikey') ||
      key.includes('password') ||
      key.includes('secret') ||
      key.includes('authorization') ||
      key.includes('credential') ||
      key.includes('accesstoken') ||
      key.includes('refreshtoken') ||
      key.includes('authtoken') ||
      key.includes('bearertoken') ||
      key.includes('privatekey') ||
      key.includes('cookie') ||
      containsSensitiveKey(child)
    )
  })
}

function reservedKeys(preset: AiProviderPreset): Set<string> {
  const keys = new Set(['model', 'messages', 'stream', 'stream_options', 'response_format'])
  if (preset === 'openai') keys.add('reasoning_effort')
  if (preset === 'gemini' || preset === 'anthropic') {
    keys.add('contents')
    keys.add('systemInstruction')
    keys.add('system')
    keys.add('max_tokens')
    keys.add('generationConfig')
  }
  if (preset === 'qwen') {
    keys.add('enable_thinking')
    keys.add('thinking_budget')
  }
  if (preset === 'deepseek') {
    keys.add('thinking')
    keys.add('reasoning_effort')
  }
  return keys
}

function parseCustomOptions(raw: string, preset: AiProviderPreset): AiCustomRequestOptions {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw || '{}')
  } catch {
    throw new AiSettingsValidationError('json')
  }
  if (parsed === null || Array.isArray(parsed) || typeof parsed !== 'object') {
    throw new AiSettingsValidationError('jsonObject')
  }
  if (new TextEncoder().encode(JSON.stringify(parsed)).byteLength > MAX_CUSTOM_OPTIONS_BYTES) {
    throw new AiSettingsValidationError('jsonTooLarge')
  }
  if (jsonDepth(parsed) > MAX_CUSTOM_OPTIONS_DEPTH) {
    throw new AiSettingsValidationError('jsonDepth')
  }
  const reserved = reservedKeys(preset)
  if (Object.keys(parsed).some((key) => reserved.has(key))) {
    throw new AiSettingsValidationError('jsonConflict')
  }
  if (containsSensitiveKey(parsed)) throw new AiSettingsValidationError('jsonSensitive')
  return parsed as AiCustomRequestOptions
}

/** Validate and normalize the shared personal/project provider settings contract. */
export function buildAiSettingsRequest(
  form: AiSettingsFormState,
  alreadyConfigured: boolean,
  searchAlreadyConfigured = false,
): AiSettingsWriteRequest {
  const baseUrl = form.base_url.trim()
  const model = form.model.trim()
  if (!baseUrl || !model) throw new AiSettingsValidationError('required')
  let parsedUrl: URL
  try {
    parsedUrl = new URL(baseUrl)
  } catch {
    throw new AiSettingsValidationError('url')
  }
  if (
    parsedUrl.protocol !== 'https:' ||
    !parsedUrl.hostname ||
    parsedUrl.username ||
    parsedUrl.password ||
    parsedUrl.search ||
    parsedUrl.hash
  ) {
    throw new AiSettingsValidationError('url')
  }
  if (model.length > 200) throw new AiSettingsValidationError('model')
  const apiKey = form.api_key.trim()
  if (!alreadyConfigured && !apiKey) throw new AiSettingsValidationError('apiKey')

  const timeout = Number(form.request_timeout_seconds)
  if (!Number.isInteger(timeout) || timeout < 30 || timeout > 600) {
    throw new AiSettingsValidationError('timeout')
  }

  let thinkingMode = form.thinking_mode
  let reasoningEffort = form.reasoning_effort
  let thinkingBudget: number | null = null
  if (form.provider_preset === 'custom') thinkingMode = 'auto'
  if (
    (form.provider_preset === 'openai' || form.provider_preset === 'gemini') &&
    reasoningEffort === 'max'
  ) {
    reasoningEffort = 'medium'
  }
  if (
    form.provider_preset === 'deepseek' &&
    reasoningEffort !== 'high' &&
    reasoningEffort !== 'max'
  ) {
    reasoningEffort = 'high'
  }
  if (form.provider_preset === 'qwen' && form.thinking_budget.trim()) {
    thinkingBudget = Number(form.thinking_budget)
    if (!Number.isInteger(thinkingBudget) || thinkingBudget < 1 || thinkingBudget > 1_000_000) {
      throw new AiSettingsValidationError('budget')
    }
  }

  const searchTimeout = Number(form.web_search_timeout_seconds)
  if (!Number.isInteger(searchTimeout) || searchTimeout < 3 || searchTimeout > 60) {
    throw new AiSettingsValidationError('searchTimeout')
  }
  const searchResults = Number(form.web_search_max_results)
  if (!Number.isInteger(searchResults) || searchResults < 1 || searchResults > 10) {
    throw new AiSettingsValidationError('searchResults')
  }
  const searchEndpoint = form.web_search_endpoint.trim()
  if (searchEndpoint) {
    let parsedSearchUrl: URL
    try {
      parsedSearchUrl = new URL(searchEndpoint)
    } catch {
      throw new AiSettingsValidationError('searchUrl')
    }
    if (
      parsedSearchUrl.protocol !== 'https:' ||
      !parsedSearchUrl.hostname ||
      parsedSearchUrl.username ||
      parsedSearchUrl.password ||
      parsedSearchUrl.search ||
      parsedSearchUrl.hash
    ) {
      throw new AiSettingsValidationError('searchUrl')
    }
  }
  if (
    (form.web_search_mode === 'adapter' ||
      (form.web_search_mode === 'auto' &&
        form.provider_preset !== 'openai' &&
        form.provider_preset !== 'gemini')) &&
    !searchAlreadyConfigured &&
    form.web_search_provider !== 'searxng' &&
    !form.web_search_api_key.trim()
  ) {
    throw new AiSettingsValidationError('searchKey')
  }

  return {
    base_url: baseUrl,
    model,
    api_key: apiKey || undefined,
    enabled: form.enabled,
    provider_preset: form.provider_preset,
    transport_mode: form.transport_mode,
    thinking_mode: thinkingMode,
    reasoning_effort: reasoningEffort,
    thinking_budget: thinkingBudget,
    request_timeout_seconds: timeout,
    custom_request_options: parseCustomOptions(form.custom_request_options, form.provider_preset),
    web_search_mode: form.web_search_mode,
    web_search_provider: form.web_search_provider,
    web_search_endpoint: searchEndpoint || null,
    web_search_api_key: form.web_search_api_key.trim() || undefined,
    web_search_timeout_seconds: searchTimeout,
    web_search_max_results: searchResults,
    web_search_citations_enabled: form.web_search_citations_enabled,
  }
}

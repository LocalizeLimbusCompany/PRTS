import { describe, expect, it } from 'vitest'

import {
  AiSettingsValidationError,
  buildAiSettingsRequest,
  createAiSettingsForm,
} from './aiSettings'

function validForm() {
  const form = createAiSettingsForm(null)
  form.base_url = 'https://api.openai.com/v1'
  form.model = 'gpt-test'
  form.api_key = 'test-key'
  return form
}

describe('AI settings validation', () => {
  it('normalizes provider-specific reasoning controls', () => {
    const deepseek = validForm()
    deepseek.provider_preset = 'deepseek'
    deepseek.thinking_mode = 'enabled'
    expect(buildAiSettingsRequest(deepseek, false).reasoning_effort).toBe('high')

    const custom = validForm()
    custom.provider_preset = 'custom'
    custom.thinking_mode = 'enabled'
    expect(buildAiSettingsRequest(custom, false).thinking_mode).toBe('auto')
  })

  it('accepts Qwen thinking budgets and the full timeout range', () => {
    const form = validForm()
    form.provider_preset = 'qwen'
    form.thinking_mode = 'enabled'
    form.thinking_budget = '4096'
    form.request_timeout_seconds = '600'
    const request = buildAiSettingsRequest(form, false)
    expect(request.thinking_budget).toBe(4096)
    expect(request.request_timeout_seconds).toBe(600)
  })

  it.each([
    ['{"model":"override"}', 'jsonConflict'],
    ['{"nested":{"authorization":"secret"}}', 'jsonSensitive'],
    ['{"apiKey":"secret"}', 'jsonSensitive'],
    ['{"nested":{"bearer-token":"secret"}}', 'jsonSensitive'],
    ['[]', 'jsonObject'],
    ['not-json', 'json'],
  ] as const)('rejects unsafe custom options %s', (raw, code) => {
    const form = validForm()
    form.custom_request_options = raw
    expect(() => buildAiSettingsRequest(form, false)).toThrowError(
      expect.objectContaining<Partial<AiSettingsValidationError>>({ code }),
    )
  })

  it('requires an API key only for the initial configuration', () => {
    const form = validForm()
    form.api_key = ''
    expect(() => buildAiSettingsRequest(form, false)).toThrowError(
      expect.objectContaining({ code: 'apiKey' }),
    )
    expect(buildAiSettingsRequest(form, true).api_key).toBeUndefined()
  })

  it('validates and retains tenant-scoped web search credentials', () => {
    const form = validForm()
    form.web_search_mode = 'adapter'
    form.web_search_endpoint = 'https://api.tavily.com/search'
    form.web_search_api_key = 'search-key'
    form.web_search_timeout_seconds = '12'
    form.web_search_max_results = '7'
    const request = buildAiSettingsRequest(form, false, false)
    expect(request.web_search_mode).toBe('adapter')
    expect(request.web_search_api_key).toBe('search-key')
    expect(request.web_search_max_results).toBe(7)

    form.web_search_api_key = ''
    expect(() => buildAiSettingsRequest(form, true, false)).toThrowError(
      expect.objectContaining({ code: 'searchKey' }),
    )
    expect(buildAiSettingsRequest(form, true, true).web_search_api_key).toBeUndefined()
  })

  it('rejects unsafe web search endpoints and bounds', () => {
    const form = validForm()
    form.web_search_mode = 'adapter'
    form.web_search_api_key = 'search-key'
    form.web_search_endpoint = 'http://localhost/search'
    expect(() => buildAiSettingsRequest(form, false, false)).toThrowError(
      expect.objectContaining({ code: 'searchUrl' }),
    )
    form.web_search_endpoint = 'https://api.tavily.com/search'
    form.web_search_max_results = '11'
    expect(() => buildAiSettingsRequest(form, false, false)).toThrowError(
      expect.objectContaining({ code: 'searchResults' }),
    )
  })
})

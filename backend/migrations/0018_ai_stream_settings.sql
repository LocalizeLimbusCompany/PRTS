-- Extend personal/project AI providers with explicit reasoning and transport controls.
-- Existing rows keep their previous request shape because `auto` emits no reasoning fields.

ALTER TABLE user_ai_settings
    ADD COLUMN provider_preset TEXT NOT NULL DEFAULT 'openai',
    ADD COLUMN thinking_mode TEXT NOT NULL DEFAULT 'auto',
    ADD COLUMN reasoning_effort TEXT NOT NULL DEFAULT 'medium',
    ADD COLUMN thinking_budget BIGINT,
    ADD COLUMN request_timeout_seconds INTEGER NOT NULL DEFAULT 180,
    ADD COLUMN custom_request_options JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD CONSTRAINT user_ai_settings_provider_preset_check
        CHECK (provider_preset IN ('openai', 'qwen', 'deepseek', 'gemini', 'custom')),
    ADD CONSTRAINT user_ai_settings_thinking_mode_check
        CHECK (thinking_mode IN ('auto', 'enabled', 'disabled')),
    ADD CONSTRAINT user_ai_settings_reasoning_effort_check
        CHECK (reasoning_effort IN ('low', 'medium', 'high', 'max')),
    ADD CONSTRAINT user_ai_settings_thinking_budget_check
        CHECK (thinking_budget IS NULL OR thinking_budget BETWEEN 1 AND 1000000),
    ADD CONSTRAINT user_ai_settings_timeout_check
        CHECK (request_timeout_seconds BETWEEN 30 AND 600),
    ADD CONSTRAINT user_ai_settings_custom_options_check
        CHECK (jsonb_typeof(custom_request_options) = 'object');

ALTER TABLE project_ai_settings
    ADD COLUMN provider_preset TEXT NOT NULL DEFAULT 'openai',
    ADD COLUMN thinking_mode TEXT NOT NULL DEFAULT 'auto',
    ADD COLUMN reasoning_effort TEXT NOT NULL DEFAULT 'medium',
    ADD COLUMN thinking_budget BIGINT,
    ADD COLUMN request_timeout_seconds INTEGER NOT NULL DEFAULT 180,
    ADD COLUMN custom_request_options JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD CONSTRAINT project_ai_settings_provider_preset_check
        CHECK (provider_preset IN ('openai', 'qwen', 'deepseek', 'gemini', 'custom')),
    ADD CONSTRAINT project_ai_settings_thinking_mode_check
        CHECK (thinking_mode IN ('auto', 'enabled', 'disabled')),
    ADD CONSTRAINT project_ai_settings_reasoning_effort_check
        CHECK (reasoning_effort IN ('low', 'medium', 'high', 'max')),
    ADD CONSTRAINT project_ai_settings_thinking_budget_check
        CHECK (thinking_budget IS NULL OR thinking_budget BETWEEN 1 AND 1000000),
    ADD CONSTRAINT project_ai_settings_timeout_check
        CHECK (request_timeout_seconds BETWEEN 30 AND 600),
    ADD CONSTRAINT project_ai_settings_custom_options_check
        CHECK (jsonb_typeof(custom_request_options) = 'object');

UPDATE workspace_foundation_state
SET schema_revision = GREATEST(schema_revision, 18),
    updated_at = now()
WHERE singleton;

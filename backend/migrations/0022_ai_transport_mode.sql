-- Select the provider transport explicitly; auto retains a one-time streaming probe/fallback.
ALTER TABLE user_ai_settings DROP CONSTRAINT user_ai_settings_provider_preset_check;
ALTER TABLE user_ai_settings ADD CONSTRAINT user_ai_settings_provider_preset_check
    CHECK (provider_preset IN ('openai', 'qwen', 'deepseek', 'gemini', 'anthropic', 'custom'));

ALTER TABLE project_ai_settings DROP CONSTRAINT project_ai_settings_provider_preset_check;
ALTER TABLE project_ai_settings ADD CONSTRAINT project_ai_settings_provider_preset_check
    CHECK (provider_preset IN ('openai', 'qwen', 'deepseek', 'gemini', 'anthropic', 'custom'));

ALTER TABLE user_ai_settings
    ADD COLUMN transport_mode TEXT NOT NULL DEFAULT 'auto',
    ADD CONSTRAINT user_ai_settings_transport_mode_chk
        CHECK (transport_mode IN ('auto', 'streaming', 'non_streaming'));

ALTER TABLE project_ai_settings
    ADD COLUMN transport_mode TEXT NOT NULL DEFAULT 'auto',
    ADD CONSTRAINT project_ai_settings_transport_mode_chk
        CHECK (transport_mode IN ('auto', 'streaming', 'non_streaming'));

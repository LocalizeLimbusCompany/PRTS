-- Add tenant-scoped optional web-search settings to personal/project AI providers.
-- Search credentials use the same application-level encryption contract as AI credentials.

ALTER TABLE user_ai_settings
    ADD COLUMN web_search_mode TEXT NOT NULL DEFAULT 'disabled',
    ADD COLUMN web_search_provider TEXT NOT NULL DEFAULT 'tavily',
    ADD COLUMN web_search_endpoint TEXT,
    ADD COLUMN web_search_api_key_ciphertext BYTEA,
    ADD COLUMN web_search_api_key_nonce BYTEA,
    ADD COLUMN web_search_api_key_hint TEXT,
    ADD COLUMN web_search_timeout_seconds INTEGER NOT NULL DEFAULT 10,
    ADD COLUMN web_search_max_results INTEGER NOT NULL DEFAULT 5,
    ADD COLUMN web_search_citations_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD CONSTRAINT user_ai_settings_web_search_mode_check
        CHECK (web_search_mode IN ('disabled', 'adapter', 'native', 'auto')),
    ADD CONSTRAINT user_ai_settings_web_search_provider_check
        CHECK (web_search_provider IN ('tavily', 'brave', 'serper', 'searxng')),
    ADD CONSTRAINT user_ai_settings_web_search_timeout_check
        CHECK (web_search_timeout_seconds BETWEEN 3 AND 60),
    ADD CONSTRAINT user_ai_settings_web_search_max_results_check
        CHECK (web_search_max_results BETWEEN 1 AND 10),
    ADD CONSTRAINT user_ai_settings_web_search_key_shape_check CHECK (
        (web_search_api_key_ciphertext IS NULL AND web_search_api_key_nonce IS NULL AND web_search_api_key_hint IS NULL)
        OR
        (web_search_api_key_ciphertext IS NOT NULL AND web_search_api_key_nonce IS NOT NULL AND web_search_api_key_hint IS NOT NULL)
    );

ALTER TABLE project_ai_settings
    ADD COLUMN web_search_mode TEXT NOT NULL DEFAULT 'disabled',
    ADD COLUMN web_search_provider TEXT NOT NULL DEFAULT 'tavily',
    ADD COLUMN web_search_endpoint TEXT,
    ADD COLUMN web_search_api_key_ciphertext BYTEA,
    ADD COLUMN web_search_api_key_nonce BYTEA,
    ADD COLUMN web_search_api_key_hint TEXT,
    ADD COLUMN web_search_timeout_seconds INTEGER NOT NULL DEFAULT 10,
    ADD COLUMN web_search_max_results INTEGER NOT NULL DEFAULT 5,
    ADD COLUMN web_search_citations_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD CONSTRAINT project_ai_settings_web_search_mode_check
        CHECK (web_search_mode IN ('disabled', 'adapter', 'native', 'auto')),
    ADD CONSTRAINT project_ai_settings_web_search_provider_check
        CHECK (web_search_provider IN ('tavily', 'brave', 'serper', 'searxng')),
    ADD CONSTRAINT project_ai_settings_web_search_timeout_check
        CHECK (web_search_timeout_seconds BETWEEN 3 AND 60),
    ADD CONSTRAINT project_ai_settings_web_search_max_results_check
        CHECK (web_search_max_results BETWEEN 1 AND 10),
    ADD CONSTRAINT project_ai_settings_web_search_key_shape_check CHECK (
        (web_search_api_key_ciphertext IS NULL AND web_search_api_key_nonce IS NULL AND web_search_api_key_hint IS NULL)
        OR
        (web_search_api_key_ciphertext IS NOT NULL AND web_search_api_key_nonce IS NOT NULL AND web_search_api_key_hint IS NOT NULL)
    );

UPDATE workspace_foundation_state
SET schema_revision = GREATEST(schema_revision, 19),
    updated_at = now()
WHERE singleton;

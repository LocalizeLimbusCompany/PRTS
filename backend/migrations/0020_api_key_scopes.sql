-- Add least-privilege API key scopes without changing the authority of existing keys.

ALTER TABLE api_keys
    ADD COLUMN scopes TEXT[] NOT NULL DEFAULT ARRAY['all']::TEXT[],
    ADD CONSTRAINT api_keys_scopes_chk CHECK (
        cardinality(scopes) > 0
        AND scopes <@ ARRAY[
            'all',
            'profile:read',
            'profile:write',
            'project:read',
            'entry:write',
            'project:write',
            'project:manage',
            'ai:use',
            'message:read',
            'message:write',
            'platform:manage'
        ]::TEXT[]
        AND (NOT ('all' = ANY(scopes)) OR cardinality(scopes) = 1)
    );

-- New users default to the character-level inline history requested by the product contract.
-- Existing saved preferences are deliberately left untouched.
ALTER TABLE users ALTER COLUMN entry_diff_mode SET DEFAULT 'character_inline';

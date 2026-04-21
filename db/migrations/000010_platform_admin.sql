ALTER TABLE users
    ADD COLUMN IF NOT EXISTS platform_role TEXT NOT NULL DEFAULT 'user',
    ADD COLUMN IF NOT EXISTS avatar_url TEXT NOT NULL DEFAULT '';

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_platform_role_check;

ALTER TABLE users
    ADD CONSTRAINT users_platform_role_check
    CHECK (platform_role IN ('owner', 'admin', 'user'));

CREATE TABLE IF NOT EXISTS platform_settings (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE,
    allow_user_create_organization BOOLEAN NOT NULL DEFAULT FALSE,
    allow_user_create_project BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (id = TRUE)
);

INSERT INTO platform_settings (id, allow_user_create_organization, allow_user_create_project)
VALUES (TRUE, FALSE, FALSE)
ON CONFLICT (id) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_users_platform_role ON users (platform_role);

-- P1：认证与账号相关表（平台级）。项目 / 成员 / 权限表见 P2。

-- 用户
CREATE TABLE users (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    username          TEXT NOT NULL UNIQUE,
    email             TEXT UNIQUE,                       -- 可空：纯 OAuth 账号可能无邮箱
    password_hash     TEXT,                              -- 可空：纯 OAuth 账号无密码
    avatar_url        TEXT,
    description       TEXT NOT NULL DEFAULT '',
    translation_langs TEXT[] NOT NULL DEFAULT '{}',      -- 个人翻译语言偏好（BCP-47）
    cp                DOUBLE PRECISION NOT NULL DEFAULT 0,
    platform_role     TEXT,                              -- NULL=普通用户；否则 super_admin|admin|maintainer
    email_verified    BOOLEAN NOT NULL DEFAULT FALSE,
    status            TEXT NOT NULL DEFAULT 'active',     -- active|pending|disabled
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_platform_role_chk
        CHECK (platform_role IN ('super_admin', 'admin', 'maintainer')),
    CONSTRAINT users_status_chk
        CHECK (status IN ('active', 'pending', 'disabled'))
);
CREATE INDEX users_platform_role_idx ON users (platform_role) WHERE platform_role IS NOT NULL;

-- 外部账号（关联：github / zoot …）
CREATE TABLE external_accounts (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id     BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    external_id TEXT NOT NULL,
    raw         JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, external_id)
);
CREATE INDEX external_accounts_user_idx ON external_accounts (user_id);

-- API Key：仅存哈希，明文在创建时返回一次
CREATE TABLE api_keys (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id      BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    key_hash     TEXT NOT NULL UNIQUE,                  -- sha256(key) 十六进制
    prefix       TEXT NOT NULL,                         -- 展示用前缀，如 prts_ab12…
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX api_keys_user_idx ON api_keys (user_id);

-- 平台运行时配置（管理后台维护）。密钥类（OAuth client_secret 等）仍走环境变量，不入此表。
CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by BIGINT REFERENCES users (id) ON DELETE SET NULL
);

-- 通用触发器：更新时自动维护 updated_at
CREATE OR REPLACE FUNCTION set_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_set_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

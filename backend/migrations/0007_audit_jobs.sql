-- Foundation：追加式审计、可恢复任务，以及 PostgreSQL 权威认证会话/outbox。
-- 本迁移发布后冻结；后续修正必须使用新的迁移号。

-- 递归检查 JSONB 对象键，阻止 token、密码和其它秘密进入审计/任务载荷。
-- 哈希只允许进入专用列（例如 auth_sessions.refresh_token_hash），不允许混入通用 payload。
CREATE OR REPLACE FUNCTION prts_jsonb_contains_secret_key(document JSONB)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    item RECORD;
    child JSONB;
    normalized_key TEXT;
BEGIN
    CASE jsonb_typeof(document)
        WHEN 'object' THEN
            FOR item IN SELECT key, value FROM jsonb_each(document)
            LOOP
                -- 先移除 snake/kebab/空格等分隔并统一小写，camelCase 也归一为同一文本。
                normalized_key := regexp_replace(lower(item.key), '[^a-z0-9]', '', 'g');
                IF normalized_key = ANY (ARRAY[
                    'token', 'accesstoken', 'refreshtoken', 'rawaccesstoken', 'rawrefreshtoken',
                    'authorization', 'password', 'passwordhash', 'secret', 'clientsecret',
                    'apikey', 'apikeyhash', 'code', 'oauthcode', 'verifier', 'codeverifier',
                    'challengeanswer'
                ]) THEN
                    RETURN TRUE;
                END IF;
                IF prts_jsonb_contains_secret_key(item.value) THEN
                    RETURN TRUE;
                END IF;
            END LOOP;
        WHEN 'array' THEN
            FOR child IN SELECT value FROM jsonb_array_elements(document)
            LOOP
                IF prts_jsonb_contains_secret_key(child) THEN
                    RETURN TRUE;
                END IF;
            END LOOP;
        ELSE
            NULL;
    END CASE;
    RETURN FALSE;
END;
$$;

-- 安全审计元数据。actor/project 字段是历史 snapshot，故不声明可级联外键。
CREATE TABLE audit_log (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    actor_id            BIGINT,
    actor_kind          TEXT NOT NULL,
    action              TEXT NOT NULL,
    target_type         TEXT NOT NULL,
    target_id           TEXT NOT NULL,
    project_id_snapshot BIGINT,
    payload             JSONB NOT NULL DEFAULT '{}',
    ip                  INET,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT audit_log_actor_kind_chk
        CHECK (actor_kind IN ('user', 'api_key', 'system', 'anonymous')),
    CONSTRAINT audit_log_payload_object_chk CHECK (jsonb_typeof(payload) = 'object'),
    CONSTRAINT audit_log_payload_secret_chk CHECK (NOT prts_jsonb_contains_secret_key(payload))
);
CREATE INDEX audit_log_actor_created_idx ON audit_log (actor_id, created_at DESC);
CREATE INDEX audit_log_project_created_idx
    ON audit_log (project_id_snapshot, created_at DESC)
    WHERE project_id_snapshot IS NOT NULL;
CREATE INDEX audit_log_target_idx ON audit_log (target_type, target_id, created_at DESC);

-- 即使应用角色被误授 UPDATE/DELETE，数据库也必须 fail closed。
CREATE OR REPLACE FUNCTION reject_audit_log_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_log_reject_update_delete
    BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION reject_audit_log_mutation();

CREATE TRIGGER audit_log_reject_truncate
    BEFORE TRUNCATE ON audit_log
    FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_log_mutation();

REVOKE UPDATE, DELETE, TRUNCATE ON audit_log FROM PUBLIC;

-- 持久化任务。project_id 在项目永久删除后置空，payload 保存清理所需 snapshot。
CREATE TABLE jobs (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind               TEXT NOT NULL,
    project_id         BIGINT REFERENCES projects (id) ON DELETE SET NULL,
    state              TEXT NOT NULL DEFAULT 'queued',
    pause_reason       TEXT,
    stage              TEXT NOT NULL DEFAULT 'queued',
    payload            JSONB NOT NULL DEFAULT '{}',
    result             JSONB,
    progress_current   BIGINT NOT NULL DEFAULT 0,
    progress_total     BIGINT,
    attempts           INTEGER NOT NULL DEFAULT 0,
    max_attempts       INTEGER NOT NULL DEFAULT 5,
    run_after          TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until        TIMESTAMPTZ,
    worker_id          TEXT,
    last_error_code    TEXT,
    last_error_message TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at         TIMESTAMPTZ,
    finished_at        TIMESTAMPTZ,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT jobs_state_chk
        CHECK (state IN ('queued', 'running', 'paused', 'succeeded', 'failed', 'cancelled')),
    CONSTRAINT jobs_pause_reason_chk CHECK (
        (state = 'paused' AND pause_reason IN ('project_pending_deletion', 'manual', 'other'))
        OR (state <> 'paused' AND pause_reason IS NULL)
    ),
    CONSTRAINT jobs_payload_object_chk CHECK (jsonb_typeof(payload) = 'object'),
    CONSTRAINT jobs_payload_secret_chk CHECK (NOT prts_jsonb_contains_secret_key(payload)),
    CONSTRAINT jobs_result_object_chk CHECK (result IS NULL OR jsonb_typeof(result) = 'object'),
    CONSTRAINT jobs_result_secret_chk
        CHECK (result IS NULL OR NOT prts_jsonb_contains_secret_key(result)),
    CONSTRAINT jobs_progress_chk CHECK (
        progress_current >= 0
        AND (progress_total IS NULL OR progress_total >= 0)
        AND (progress_total IS NULL OR progress_current <= progress_total)
    ),
    CONSTRAINT jobs_attempts_chk CHECK (attempts >= 0 AND max_attempts >= 1)
);
CREATE INDEX jobs_claim_idx ON jobs (run_after, id)
    WHERE state = 'queued';
CREATE INDEX jobs_expired_lease_idx ON jobs (lease_until, id)
    WHERE state = 'running';
CREATE INDEX jobs_project_idx ON jobs (project_id, id DESC)
    WHERE project_id IS NOT NULL;
CREATE TRIGGER jobs_set_updated_at
    BEFORE UPDATE ON jobs FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- PostgreSQL 权威会话状态；Redis 只能缓存 active lookup 或承载不可认证 pending material。
CREATE TABLE auth_sessions (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_handle     TEXT NOT NULL UNIQUE,
    family_handle      TEXT NOT NULL,
    user_id            BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    refresh_token_hash TEXT NOT NULL UNIQUE,
    state              TEXT NOT NULL DEFAULT 'pending',
    expires_at         TIMESTAMPTZ NOT NULL,
    predecessor_id     BIGINT UNIQUE REFERENCES auth_sessions (id) ON DELETE SET NULL,
    successor_id       BIGINT UNIQUE REFERENCES auth_sessions (id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT auth_sessions_state_chk
        CHECK (state IN ('pending', 'active', 'rotating', 'revoked', 'expired')),
    CONSTRAINT auth_sessions_handles_chk
        CHECK (length(session_handle) >= 16 AND length(family_handle) >= 16),
    CONSTRAINT auth_sessions_refresh_hash_chk
        CHECK (refresh_token_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT auth_sessions_not_self_linked_chk
        CHECK (predecessor_id IS DISTINCT FROM id AND successor_id IS DISTINCT FROM id)
);
CREATE INDEX auth_sessions_user_family_idx ON auth_sessions (user_id, family_handle);
CREATE UNIQUE INDEX auth_sessions_one_active_family_idx
    ON auth_sessions (user_id, family_handle)
    WHERE state = 'active';
CREATE INDEX auth_sessions_expiry_idx ON auth_sessions (expires_at)
    WHERE state IN ('pending', 'active', 'rotating');

-- 权威状态机在数据库边界同样 fail closed，不能靠绕过仓储复活 revoked/expired 会话。
CREATE OR REPLACE FUNCTION enforce_auth_session_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    -- 带 predecessor 的 pending successor 不得通过清空链接伪装成普通签发。
    IF OLD.state = 'pending'
        AND OLD.predecessor_id IS NOT NULL
        AND NEW.predecessor_id IS DISTINCT FROM OLD.predecessor_id THEN
        RAISE EXCEPTION 'pending auth session predecessor linkage is immutable'
            USING ERRCODE = '23514';
    END IF;
    -- complete_rotation 设置反向链接时，successor 必须已精确指回当前 predecessor。
    IF NEW.successor_id IS DISTINCT FROM OLD.successor_id
        AND NEW.successor_id IS NOT NULL
        AND NOT (
            OLD.state = 'rotating'
            AND NEW.state = 'revoked'
            AND EXISTS (
                SELECT 1
                FROM auth_sessions AS successor
                WHERE successor.id = NEW.successor_id
                  AND successor.predecessor_id = OLD.id
                  AND successor.user_id = OLD.user_id
                  AND successor.family_handle = OLD.family_handle
                  AND successor.state = 'pending'
                  AND successor.expires_at > now()
            )
        ) THEN
        RAISE EXCEPTION 'invalid auth session successor linkage' USING ERRCODE = '23514';
    END IF;
    IF NEW.state = OLD.state THEN
        RETURN NEW;
    END IF;
    IF OLD.state = 'pending' AND NEW.state = 'active' THEN
        IF NEW.expires_at <= now() THEN
            RAISE EXCEPTION 'expired auth session cannot become active' USING ERRCODE = '23514';
        END IF;
        IF NEW.predecessor_id IS NULL THEN
            RETURN NEW;
        END IF;
        -- Rotation 必须先撤销精确 predecessor 并建立反向链接，再激活 successor。
        IF EXISTS (
            SELECT 1
            FROM auth_sessions AS predecessor
            WHERE predecessor.id = NEW.predecessor_id
              AND predecessor.successor_id = NEW.id
              AND predecessor.user_id = NEW.user_id
              AND predecessor.family_handle = NEW.family_handle
              AND predecessor.state = 'revoked'
              AND predecessor.expires_at > now()
        ) THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'linked pending auth session requires completed rotation'
            USING ERRCODE = '23514';
    END IF;
    IF (OLD.state = 'pending' AND NEW.state IN ('revoked', 'expired'))
        OR (OLD.state = 'active' AND NEW.state IN ('rotating', 'revoked', 'expired'))
        OR (OLD.state = 'rotating' AND NEW.state IN ('revoked', 'expired')) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'invalid auth session transition: % -> %', OLD.state, NEW.state
        USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER auth_sessions_enforce_transition
    BEFORE UPDATE OF state ON auth_sessions
    FOR EACH ROW EXECUTE FUNCTION enforce_auth_session_transition();
CREATE TRIGGER auth_sessions_set_updated_at
    BEFORE UPDATE ON auth_sessions FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Durable auth intent/outbox。只携带 opaque handle；任何 raw token 均由 JSON 约束拒绝。
CREATE TABLE auth_session_intents (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_id         BIGINT NOT NULL REFERENCES auth_sessions (id) ON DELETE CASCADE,
    kind               TEXT NOT NULL,
    state              TEXT NOT NULL DEFAULT 'queued',
    payload            JSONB NOT NULL DEFAULT '{}',
    attempts           INTEGER NOT NULL DEFAULT 0,
    max_attempts       INTEGER NOT NULL DEFAULT 5,
    run_after          TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until        TIMESTAMPTZ,
    worker_id          TEXT,
    last_error_code    TEXT,
    last_error_message TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at       TIMESTAMPTZ,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT auth_session_intents_state_chk
        CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    CONSTRAINT auth_session_intents_payload_object_chk CHECK (jsonb_typeof(payload) = 'object'),
    CONSTRAINT auth_session_intents_payload_secret_chk
        CHECK (NOT prts_jsonb_contains_secret_key(payload)),
    CONSTRAINT auth_session_intents_attempts_chk CHECK (attempts >= 0 AND max_attempts >= 1)
);
CREATE INDEX auth_session_intents_claim_idx ON auth_session_intents (run_after, id)
    WHERE state = 'queued';
CREATE INDEX auth_session_intents_expired_lease_idx
    ON auth_session_intents (lease_until, id)
    WHERE state = 'running';
CREATE INDEX auth_session_intents_session_idx ON auth_session_intents (session_id, id);
CREATE TRIGGER auth_session_intents_set_updated_at
    BEFORE UPDATE ON auth_session_intents FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Migration owner 与 runtime role 是真实安全边界。runtime role 名由 migrator 在同一连接
-- 通过 set_config 传入；动态标识符只能经 format('%I') 引用，禁止拼接未转义 SQL。
DO $$
DECLARE
    runtime_role TEXT := current_setting('prts.runtime_role', true);
    migration_role TEXT := current_user;
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    IF runtime_role = migration_role THEN
        RAISE EXCEPTION 'migration owner and runtime role must be distinct';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = runtime_role) THEN
        RAISE EXCEPTION 'runtime role % does not exist', runtime_role;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = runtime_role AND rolsuper) THEN
        RAISE EXCEPTION 'runtime role % must not be superuser', runtime_role;
    END IF;

    REVOKE CREATE ON SCHEMA public FROM PUBLIC;
    EXECUTE format('REVOKE CREATE ON SCHEMA public FROM %I', runtime_role);
    EXECUTE format('GRANT USAGE ON SCHEMA public TO %I', runtime_role);

    -- 既有业务仓储需要 CRUD，但不授予 TRUNCATE/REFERENCES/TRIGGER/DDL。
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO %I',
        runtime_role
    );
    EXECUTE format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public '
        'GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO %I',
        migration_role,
        runtime_role
    );
    EXECUTE format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public '
        'GRANT USAGE, SELECT ON SEQUENCES TO %I',
        migration_role,
        runtime_role
    );

    IF to_regclass('public._sqlx_migrations') IS NOT NULL THEN
        EXECUTE format(
            'REVOKE ALL PRIVILEGES ON TABLE public._sqlx_migrations FROM %I',
            runtime_role
        );
    END IF;

    -- audit_log 是特例：owner/trigger 仍属于 migrator，runtime 只能追加和读取。
    EXECUTE format('REVOKE ALL PRIVILEGES ON TABLE audit_log FROM %I', runtime_role);
    EXECUTE format('GRANT SELECT, INSERT ON TABLE audit_log TO %I', runtime_role);
END;
$$;

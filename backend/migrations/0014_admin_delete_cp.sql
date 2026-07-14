-- Stage 7 foundation: persistent password reminders, exact-tenths CP, and the complete
-- delayed project-deletion schema consumed by Task 7.3. This is the only Stage 7 migration.

-- Reject legacy values that cannot be represented safely before replacing the float column.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM users
        WHERE cp::TEXT IN ('NaN', 'Infinity', '-Infinity')
           OR cp < 0
           OR cp > 922337203685477500.0
    ) THEN
        RAISE EXCEPTION 'users.cp contains a value that cannot be converted to non-negative tenths';
    END IF;
END;
$$;

ALTER TABLE users
    ADD COLUMN password_change_required BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN cp_tenths BIGINT NOT NULL DEFAULT 0;

-- One cp_tenths unit is exactly 0.1 CP. Round the legacy binary float once, then remove it.
UPDATE users
SET cp_tenths = ROUND(cp * 10)::BIGINT;

ALTER TABLE users
    DROP COLUMN cp,
    ADD CONSTRAINT users_cp_tenths_nonnegative_chk CHECK (cp_tenths >= 0),
    ADD CONSTRAINT users_password_change_required_hash_chk CHECK (
        NOT password_change_required OR password_hash IS NOT NULL
    );

ALTER TABLE memberships
    ADD COLUMN cp_tenths BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT memberships_cp_tenths_nonnegative_chk CHECK (cp_tenths >= 0);

-- Administrator user lists use signed keyset cursors for every sort and literal substring search.
CREATE INDEX users_admin_username_cursor_idx
    ON users (lower(username), id);
CREATE INDEX users_admin_created_at_cursor_idx
    ON users (created_at, id);
CREATE INDEX users_admin_role_username_cursor_idx
    ON users (platform_role, lower(username), id);
CREATE INDEX users_admin_role_created_at_cursor_idx
    ON users (platform_role, created_at, id);
CREATE INDEX users_admin_username_trgm_idx
    ON users USING GIN (lower(username) gin_trgm_ops);

-- Task 7.3 consumes these fields without modifying this migration. Both cross-links use
-- SET NULL, so projects and purge jobs never form a bidirectional cascade.
ALTER TABLE projects
    ADD COLUMN deletion_scheduled_at TIMESTAMPTZ,
    ADD COLUMN deletion_requested_by BIGINT,
    ADD COLUMN deletion_job_id BIGINT,
    ADD CONSTRAINT projects_deletion_requested_by_fkey
        FOREIGN KEY (deletion_requested_by) REFERENCES users (id) ON DELETE SET NULL,
    ADD CONSTRAINT projects_deletion_job_id_fkey
        FOREIGN KEY (deletion_job_id) REFERENCES jobs (id) ON DELETE SET NULL,
    ADD CONSTRAINT projects_deletion_state_chk CHECK (
        (deletion_scheduled_at IS NULL
            AND deletion_requested_by IS NULL
            AND deletion_job_id IS NULL)
        OR deletion_scheduled_at IS NOT NULL
    );

-- A purge job can be bound to at most one live project. The FK remains nullable because the
-- job is intentionally retained after DB-first project deletion.
CREATE UNIQUE INDEX projects_deletion_job_unique_idx
    ON projects (deletion_job_id)
    WHERE deletion_job_id IS NOT NULL;

-- Owner countdown/status queries and deadline scans use the same stable deadline/id order.
CREATE INDEX projects_pending_deletion_idx
    ON projects (deletion_scheduled_at, id)
    WHERE deletion_scheduled_at IS NOT NULL;

-- Dedicated claim path for both the initial deadline and idempotent external-cleanup retries.
CREATE INDEX jobs_project_purge_claim_idx
    ON jobs (run_after, id)
    WHERE state = 'queued' AND kind = 'project_purge';

UPDATE workspace_foundation_state
SET schema_revision = GREATEST(schema_revision, 14),
    updated_at = now()
WHERE singleton;

-- These columns are added after the original table grants. PostgreSQL column privileges are
-- covered by table privileges, but repeat the explicit runtime grants to keep rollout auditable.
DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON users, memberships, projects, jobs TO %I',
        runtime_role
    );
END;
$$;

-- P6 contribution scoring: immutable event ledger for UTC week/month leaderboards.
-- users.cp_tenths and memberships.cp_tenths remain the all-time aggregate sources.

CREATE TABLE contribution_events (
    id            BIGSERIAL PRIMARY KEY,
    user_id       BIGINT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    project_id    BIGINT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    entry_id      BIGINT REFERENCES entries(id) ON DELETE SET NULL,
    entry_version BIGINT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('edit', 'review')),
    distance      BIGINT NOT NULL CHECK (distance >= 0),
    cp_tenths     BIGINT NOT NULL CHECK (cp_tenths > 0),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (entry_id, entry_version)
);

CREATE INDEX contribution_events_platform_period_rank_idx
    ON contribution_events (created_at, user_id) INCLUDE (cp_tenths);
CREATE INDEX contribution_events_project_user_idx
    ON contribution_events (project_id, user_id, created_at) INCLUDE (cp_tenths);

-- Rejoining a project restores the user's project aggregate from the immutable event ledger.
CREATE OR REPLACE FUNCTION prts_membership_restore_cp()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.cp_tenths := COALESCE((
        SELECT sum(event.cp_tenths)
        FROM contribution_events AS event
        WHERE event.project_id = NEW.project_id
          AND event.user_id = NEW.user_id
    ), 0);
    RETURN NEW;
END;
$$;

CREATE TRIGGER memberships_restore_cp_trg
BEFORE INSERT ON memberships
FOR EACH ROW EXECUTE FUNCTION prts_membership_restore_cp();

UPDATE workspace_foundation_state
SET schema_revision = GREATEST(schema_revision, 15),
    updated_at = now()
WHERE singleton;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, DELETE ON contribution_events TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON SEQUENCE contribution_events_id_seq TO %I',
        runtime_role
    );
END;
$$;

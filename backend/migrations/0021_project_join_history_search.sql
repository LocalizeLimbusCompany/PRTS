-- Project self-service membership, project-wide entry history, and all-source lexical search.

ALTER TABLE projects
    ADD COLUMN join_policy TEXT NOT NULL DEFAULT 'admin_only',
    ADD COLUMN join_default_role TEXT NOT NULL DEFAULT 'translator',
    ADD COLUMN history_visibility TEXT NOT NULL DEFAULT 'viewers',
    ADD COLUMN join_password_hash TEXT,
    ADD COLUMN join_quiz_question TEXT,
    ADD COLUMN join_quiz_answer_hash TEXT,
    ADD CONSTRAINT projects_join_policy_chk CHECK (
        join_policy IN ('application', 'free', 'admin_only', 'password', 'quiz')
    ),
    ADD CONSTRAINT projects_join_default_role_chk CHECK (
        join_default_role IN ('translator', 'reviewer')
    ),
    ADD CONSTRAINT projects_history_visibility_chk CHECK (
        history_visibility IN ('viewers', 'members', 'managers')
    );

CREATE TABLE project_join_applications (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id  BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    user_id     BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status      TEXT NOT NULL DEFAULT 'pending',
    message     TEXT NOT NULL DEFAULT '',
    decided_by  BIGINT REFERENCES users (id) ON DELETE SET NULL,
    decided_role TEXT,
    decided_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_join_applications_status_chk CHECK (
        status IN ('pending', 'approved', 'rejected', 'withdrawn')
    ),
    CONSTRAINT project_join_applications_role_chk CHECK (
        decided_role IS NULL OR decided_role IN ('manager', 'reviewer', 'translator')
    )
);
CREATE UNIQUE INDEX project_join_applications_pending_unique
    ON project_join_applications (project_id, user_id) WHERE status = 'pending';
CREATE INDEX project_join_applications_project_pending_idx
    ON project_join_applications (project_id, id DESC) WHERE status = 'pending';
CREATE TRIGGER project_join_applications_set_updated_at
    BEFORE UPDATE ON project_join_applications
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Denormalized project ownership makes project history a bounded keyset scan instead of a scan
-- across every project's version table. A trigger keeps all existing writers compatible.
ALTER TABLE entry_versions
    ADD COLUMN project_id BIGINT REFERENCES projects (id) ON DELETE RESTRICT,
    ADD COLUMN locked BOOLEAN,
    ADD COLUMN hidden BOOLEAN;

UPDATE entry_versions AS version
SET project_id = entry.project_id,
    locked = COALESCE(version.locked, entry.locked),
    hidden = COALESCE(version.hidden, entry.hidden)
FROM entries AS entry
WHERE entry.id = version.entry_id;

ALTER TABLE entry_versions
    ALTER COLUMN project_id SET NOT NULL,
    ALTER COLUMN locked SET NOT NULL,
    ALTER COLUMN hidden SET NOT NULL;

CREATE FUNCTION entry_versions_complete_snapshot() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE current_entry entries%ROWTYPE;
BEGIN
    SELECT * INTO current_entry FROM entries WHERE id = NEW.entry_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'entry version target does not exist' USING ERRCODE = '23503';
    END IF;
    NEW.project_id := current_entry.project_id;
    NEW.locked := COALESCE(NEW.locked, current_entry.locked);
    NEW.hidden := COALESCE(NEW.hidden, current_entry.hidden);
    RETURN NEW;
END;
$$;

CREATE TRIGGER entry_versions_complete_snapshot_trg
    BEFORE INSERT ON entry_versions
    FOR EACH ROW EXECUTE FUNCTION entry_versions_complete_snapshot();
CREATE INDEX entry_versions_project_history_idx
    ON entry_versions (project_id, created_at DESC, id DESC);

-- Aggregate all original-language values once at write time. Hot searches never expand JSONB.
ALTER TABLE entries
    ADD COLUMN source_all_text TEXT NOT NULL DEFAULT '',
    ADD COLUMN source_all_tsv TSVECTOR;

CREATE OR REPLACE FUNCTION entries_search_maintain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    source_language TEXT;
    target_language TEXT;
    repair_state TEXT;
    next_source TEXT;
    next_source_all TEXT;
BEGIN
    SELECT primary_source_lang, target_lang, language_repair_state
      INTO source_language, target_language, repair_state
      FROM projects
     WHERE id = NEW.project_id;

    SELECT COALESCE(string_agg(value, E'\n' ORDER BY key), '')
      INTO next_source_all
      FROM jsonb_each_text(NEW.original);
    NEW.source_all_text := next_source_all;
    NEW.source_all_tsv := to_tsvector('simple'::regconfig, next_source_all);

    IF repair_state <> 'ready' OR source_language IS NULL THEN
        NEW.source_text := '';
        NEW.source_tsv := NULL;
        NEW.translation_tsv := NULL;
        NEW.embedding := NULL;
        NEW.embed_attempts := 0;
        RETURN NEW;
    END IF;
    IF NOT (NEW.original ? source_language) THEN
        RAISE EXCEPTION 'entry original lacks canonical primary source language'
            USING ERRCODE = '23514';
    END IF;
    next_source := COALESCE(NEW.original ->> source_language, '');
    IF TG_OP = 'INSERT' OR next_source IS DISTINCT FROM OLD.source_text THEN
        NEW.embedding := NULL;
        NEW.embed_attempts := 0;
    END IF;
    NEW.source_text := next_source;
    NEW.source_tsv := to_tsvector(prts_ts_config(source_language), next_source);
    NEW.translation_tsv := to_tsvector(
        prts_ts_config(target_language), COALESCE(NEW.translation, '')
    );
    RETURN NEW;
END;
$$;

UPDATE entries SET original = original;
CREATE INDEX entries_source_all_tsv_idx ON entries USING GIN (source_all_tsv);
CREATE INDEX entries_source_all_trgm_idx ON entries USING GIN (source_all_text gin_trgm_ops);

CREATE INDEX terms_source_text_trgm_idx ON terms USING GIN (source_text gin_trgm_ops);
CREATE INDEX terms_translation_trgm_idx ON terms USING GIN (translation gin_trgm_ops);
CREATE INDEX terms_notes_trgm_idx ON terms USING GIN (notes gin_trgm_ops);
CREATE INDEX users_username_trgm_idx ON users USING GIN (username gin_trgm_ops);

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON project_join_applications TO %I', runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON SEQUENCE project_join_applications_id_seq TO %I', runtime_role
    );
    EXECUTE format(
        'GRANT EXECUTE ON FUNCTION entry_versions_complete_snapshot() TO %I', runtime_role
    );
END;
$$;

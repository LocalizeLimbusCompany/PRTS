-- Workspace foundation：语言修复边界、项目元信息、唯一 owner 与物化统计。
-- 本迁移只预建删除字段；soft-delete writer 与 history schema 必须等 0010。

ALTER TABLE projects
    ADD COLUMN primary_source_lang TEXT,
    ADD COLUMN language_repair_state TEXT NOT NULL DEFAULT 'repairing',
    ADD COLUMN language_repair_job_id BIGINT REFERENCES jobs (id) ON DELETE SET NULL,
    ADD COLUMN primary_source_changed_at TIMESTAMPTZ,
    ADD COLUMN lexical_state TEXT NOT NULL DEFAULT 'pending',
    ADD COLUMN lexical_job_id BIGINT REFERENCES jobs (id) ON DELETE SET NULL,
    ADD COLUMN embedding_state TEXT NOT NULL DEFAULT 'pending',
    ADD COLUMN embedding_job_id BIGINT REFERENCES jobs (id) ON DELETE SET NULL,
    ADD COLUMN avatar_key TEXT,
    ADD COLUMN avatar_content_type TEXT,
    ADD COLUMN avatar_updated_at TIMESTAMPTZ,
    ADD CONSTRAINT projects_language_repair_state_chk
        CHECK (language_repair_state IN ('repairing', 'ready', 'needs_language_resolution')),
    ADD CONSTRAINT projects_lexical_state_chk
        CHECK (lexical_state IN ('pending', 'ready', 'rebuilding', 'failed')),
    ADD CONSTRAINT projects_embedding_state_chk
        CHECK (embedding_state IN ('pending', 'running', 'ready', 'degraded', 'failed')),
    ADD CONSTRAINT projects_ready_language_chk CHECK (
        language_repair_state <> 'ready'
        OR (
            primary_source_lang IS NOT NULL
            AND cardinality(source_langs) > 0
            AND primary_source_lang = ANY(source_langs)
        )
    ),
    ADD CONSTRAINT projects_avatar_metadata_chk CHECK (
        (avatar_key IS NULL AND avatar_content_type IS NULL AND avatar_updated_at IS NULL)
        OR (avatar_key IS NOT NULL AND avatar_content_type = 'image/webp' AND avatar_updated_at IS NOT NULL)
    );

ALTER TABLE folders
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN deleted_by BIGINT REFERENCES users (id) ON DELETE SET NULL,
    ADD COLUMN deletion_change_set_id UUID;

ALTER TABLE files
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN deleted_by BIGINT REFERENCES users (id) ON DELETE SET NULL,
    ADD COLUMN deletion_change_set_id UUID;

ALTER TABLE entries
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN deleted_by BIGINT REFERENCES users (id) ON DELETE SET NULL,
    ADD COLUMN deletion_change_set_id UUID;

CREATE OR REPLACE FUNCTION initialize_project_language_foundation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.primary_source_lang IS NULL AND cardinality(NEW.source_langs) > 0 THEN
        NEW.primary_source_lang := NEW.source_langs[1];
    END IF;
    IF NEW.primary_source_lang IS NOT NULL
       AND NEW.primary_source_lang = ANY(NEW.source_langs) THEN
        NEW.language_repair_state := 'ready';
        NEW.lexical_state := 'ready';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER projects_language_foundation_init_trg
    BEFORE INSERT ON projects
    FOR EACH ROW EXECUTE FUNCTION initialize_project_language_foundation();

CREATE INDEX folders_project_active_path_idx
    ON folders (project_id, path) WHERE deleted_at IS NULL;
CREATE INDEX files_project_active_path_idx
    ON files (project_id, path, id) WHERE deleted_at IS NULL;
CREATE INDEX entries_project_active_id_idx
    ON entries (project_id, id) WHERE deleted_at IS NULL;

-- issue 仅保存 tag 与不可变实体标识；冲突正文仍留在原业务行，不能复制到诊断表。
CREATE TABLE language_resolution_issues (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id         BIGINT REFERENCES projects (id) ON DELETE SET NULL,
    entry_id           BIGINT REFERENCES entries (id) ON DELETE SET NULL,
    user_id            BIGINT REFERENCES users (id) ON DELETE SET NULL,
    entity_type        TEXT NOT NULL,
    entity_id_snapshot TEXT NOT NULL,
    issue_kind         TEXT NOT NULL,
    raw_tag            TEXT,
    canonical_tag      TEXT,
    metadata           JSONB NOT NULL DEFAULT '{}',
    resolved_at        TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT language_resolution_entity_chk
        CHECK (entity_type IN ('user', 'project', 'entry', 'term')),
    CONSTRAINT language_resolution_kind_chk CHECK (
        issue_kind IN (
            'invalid_tag', 'canonical_duplicate', 'conflicting_original_keys',
            'empty_source_languages', 'primary_not_in_sources'
        )
    ),
    CONSTRAINT language_resolution_metadata_chk
        CHECK (jsonb_typeof(metadata) = 'object' AND NOT prts_jsonb_contains_secret_key(metadata))
);
CREATE INDEX language_resolution_project_open_idx
    ON language_resolution_issues (project_id, id) WHERE resolved_at IS NULL;
CREATE INDEX language_resolution_user_open_idx
    ON language_resolution_issues (user_id, id) WHERE resolved_at IS NULL;
CREATE UNIQUE INDEX language_resolution_open_entity_kind_idx
    ON language_resolution_issues (entity_type, entity_id_snapshot, issue_kind)
    WHERE resolved_at IS NULL;

-- 以 projects.owner_id 为唯一真值修复历史 membership。
UPDATE memberships AS membership
SET role = 'manager'
FROM projects AS project
WHERE membership.project_id = project.id
  AND membership.role = 'owner'
  AND membership.user_id <> project.owner_id;

INSERT INTO memberships (project_id, user_id, role)
SELECT id, owner_id, 'owner' FROM projects
ON CONFLICT (project_id, user_id) DO UPDATE SET role = 'owner';

INSERT INTO audit_log (
    actor_id, actor_kind, action, target_type, target_id, project_id_snapshot, payload
)
SELECT NULL, 'system', 'project.owner_membership_repaired', 'project', id::TEXT, id,
       jsonb_build_object('owner_id', owner_id)
FROM projects;

INSERT INTO notifications (user_id, type, payload)
SELECT owner_id, 'project_owner_membership_repaired', jsonb_build_object('project_id', id)
FROM projects;

CREATE UNIQUE INDEX memberships_one_owner_idx
    ON memberships (project_id) WHERE role = 'owner';

CREATE OR REPLACE FUNCTION create_project_owner_membership()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO memberships (project_id, user_id, role)
    VALUES (NEW.id, NEW.owner_id, 'owner')
    ON CONFLICT (project_id, user_id) DO UPDATE SET role = 'owner';
    RETURN NEW;
END;
$$;

CREATE TRIGGER projects_owner_membership_init_trg
    AFTER INSERT ON projects
    FOR EACH ROW EXECUTE FUNCTION create_project_owner_membership();

CREATE OR REPLACE FUNCTION enforce_project_owner_membership()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE expected_owner BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN
            RETURN OLD;
        END IF;
        IF OLD.role = 'owner' THEN
            SELECT owner_id INTO expected_owner FROM projects WHERE id = OLD.project_id;
            IF expected_owner = OLD.user_id THEN
                RAISE EXCEPTION 'project owner membership cannot be removed' USING ERRCODE = '23514';
            END IF;
        END IF;
        RETURN OLD;
    END IF;
    IF TG_OP = 'UPDATE' AND OLD.role = 'owner' AND NEW.role <> 'owner' THEN
        SELECT owner_id INTO expected_owner FROM projects WHERE id = OLD.project_id;
        IF expected_owner = OLD.user_id THEN
            RAISE EXCEPTION 'project owner membership cannot be downgraded' USING ERRCODE = '23514';
        END IF;
    END IF;
    IF NEW.role = 'owner' THEN
        SELECT owner_id INTO expected_owner FROM projects WHERE id = NEW.project_id;
        IF expected_owner IS DISTINCT FROM NEW.user_id THEN
            RAISE EXCEPTION 'owner membership must match projects.owner_id' USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER memberships_owner_guard_trg
    AFTER INSERT OR UPDATE OR DELETE ON memberships
    DEFERRABLE INITIALLY IMMEDIATE
    FOR EACH ROW EXECUTE FUNCTION enforce_project_owner_membership();

CREATE OR REPLACE FUNCTION prevent_project_owner_transfer()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.owner_id IS DISTINCT FROM OLD.owner_id THEN
        RAISE EXCEPTION 'project owner transfer is not available' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER projects_owner_transfer_guard_trg
    BEFORE UPDATE OF owner_id ON projects
    FOR EACH ROW EXECUTE FUNCTION prevent_project_owner_transfer();

-- effective-visible 统计真值。删除字段在 foundation 期间必须保持 NULL，谓词已为 0010 预留。
CREATE TABLE project_stats (
    project_id          BIGINT PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    visible_total       BIGINT NOT NULL DEFAULT 0,
    untranslated_count  BIGINT NOT NULL DEFAULT 0,
    translated_count    BIGINT NOT NULL DEFAULT 0,
    questioned_count    BIGINT NOT NULL DEFAULT 0,
    checked_count       BIGINT NOT NULL DEFAULT 0,
    reviewed_count      BIGINT NOT NULL DEFAULT 0,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_stats_nonnegative_chk CHECK (
        visible_total >= 0 AND untranslated_count >= 0 AND translated_count >= 0
        AND questioned_count >= 0 AND checked_count >= 0 AND reviewed_count >= 0
    ),
    CONSTRAINT project_stats_sum_chk CHECK (
        visible_total = untranslated_count + translated_count + questioned_count
            + checked_count + reviewed_count
    )
);

CREATE TABLE file_stats (
    file_id             BIGINT PRIMARY KEY REFERENCES files (id) ON DELETE CASCADE,
    project_id          BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    visible_total       BIGINT NOT NULL DEFAULT 0,
    untranslated_count  BIGINT NOT NULL DEFAULT 0,
    translated_count    BIGINT NOT NULL DEFAULT 0,
    questioned_count    BIGINT NOT NULL DEFAULT 0,
    checked_count       BIGINT NOT NULL DEFAULT 0,
    reviewed_count      BIGINT NOT NULL DEFAULT 0,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT file_stats_nonnegative_chk CHECK (
        visible_total >= 0 AND untranslated_count >= 0 AND translated_count >= 0
        AND questioned_count >= 0 AND checked_count >= 0 AND reviewed_count >= 0
    ),
    CONSTRAINT file_stats_sum_chk CHECK (
        visible_total = untranslated_count + translated_count + questioned_count
            + checked_count + reviewed_count
    )
);
CREATE INDEX file_stats_project_idx ON file_stats (project_id, file_id);

CREATE OR REPLACE FUNCTION prts_stats_state_delta(state_name TEXT, amount BIGINT)
RETURNS TABLE (
    untranslated BIGINT, translated BIGINT, questioned BIGINT, checked BIGINT, reviewed BIGINT
)
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
    SELECT
        CASE WHEN state_name = 'untranslated' THEN amount ELSE 0 END,
        CASE WHEN state_name = 'translated' THEN amount ELSE 0 END,
        CASE WHEN state_name = 'questioned' THEN amount ELSE 0 END,
        CASE WHEN state_name = 'checked' THEN amount ELSE 0 END,
        CASE WHEN state_name = 'reviewed' THEN amount ELSE 0 END
$$;

CREATE OR REPLACE FUNCTION prts_apply_entry_stats_delta(
    target_project_id BIGINT, target_file_id BIGINT, state_name TEXT, amount BIGINT
)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE delta RECORD;
BEGIN
    SELECT * INTO delta FROM prts_stats_state_delta(state_name, amount);

    IF amount < 0 THEN
        UPDATE project_stats SET
            visible_total = GREATEST(0, visible_total + amount),
            untranslated_count = GREATEST(0, untranslated_count + delta.untranslated),
            translated_count = GREATEST(0, translated_count + delta.translated),
            questioned_count = GREATEST(0, questioned_count + delta.questioned),
            checked_count = GREATEST(0, checked_count + delta.checked),
            reviewed_count = GREATEST(0, reviewed_count + delta.reviewed),
            updated_at = now()
        WHERE project_id = target_project_id;

        UPDATE file_stats SET
            visible_total = GREATEST(0, visible_total + amount),
            untranslated_count = GREATEST(0, untranslated_count + delta.untranslated),
            translated_count = GREATEST(0, translated_count + delta.translated),
            questioned_count = GREATEST(0, questioned_count + delta.questioned),
            checked_count = GREATEST(0, checked_count + delta.checked),
            reviewed_count = GREATEST(0, reviewed_count + delta.reviewed),
            updated_at = now()
        WHERE file_id = target_file_id;
        RETURN;
    END IF;

    INSERT INTO project_stats (
        project_id, visible_total, untranslated_count, translated_count,
        questioned_count, checked_count, reviewed_count
    ) VALUES (
        target_project_id, amount, delta.untranslated, delta.translated,
        delta.questioned, delta.checked, delta.reviewed
    )
    ON CONFLICT (project_id) DO UPDATE SET
        visible_total = GREATEST(0, project_stats.visible_total + EXCLUDED.visible_total),
        untranslated_count = GREATEST(
            0, project_stats.untranslated_count + EXCLUDED.untranslated_count
        ),
        translated_count = GREATEST(
            0, project_stats.translated_count + EXCLUDED.translated_count
        ),
        questioned_count = GREATEST(
            0, project_stats.questioned_count + EXCLUDED.questioned_count
        ),
        checked_count = GREATEST(0, project_stats.checked_count + EXCLUDED.checked_count),
        reviewed_count = GREATEST(0, project_stats.reviewed_count + EXCLUDED.reviewed_count),
        updated_at = now();

    INSERT INTO file_stats (
        file_id, project_id, visible_total, untranslated_count, translated_count,
        questioned_count, checked_count, reviewed_count
    ) VALUES (
        target_file_id, target_project_id, amount, delta.untranslated, delta.translated,
        delta.questioned, delta.checked, delta.reviewed
    )
    ON CONFLICT (file_id) DO UPDATE SET
        visible_total = GREATEST(0, file_stats.visible_total + EXCLUDED.visible_total),
        untranslated_count = GREATEST(
            0, file_stats.untranslated_count + EXCLUDED.untranslated_count
        ),
        translated_count = GREATEST(0, file_stats.translated_count + EXCLUDED.translated_count),
        questioned_count = GREATEST(0, file_stats.questioned_count + EXCLUDED.questioned_count),
        checked_count = GREATEST(0, file_stats.checked_count + EXCLUDED.checked_count),
        reviewed_count = GREATEST(0, file_stats.reviewed_count + EXCLUDED.reviewed_count),
        updated_at = now();
END;
$$;

CREATE OR REPLACE FUNCTION prts_entry_is_effectively_visible(entry_row entries)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
AS $$
    SELECT entry_row.deleted_at IS NULL
       AND NOT entry_row.hidden
       AND EXISTS (
           SELECT 1 FROM files AS file
           WHERE file.id = entry_row.file_id AND file.deleted_at IS NULL
       )
       AND NOT EXISTS (
           SELECT 1
           FROM files AS file
           JOIN folders AS ancestor
             ON ancestor.project_id = file.project_id
            AND (file.path LIKE ancestor.path || '/%' OR file.path = ancestor.path)
           WHERE file.id = entry_row.file_id AND ancestor.deleted_at IS NOT NULL
       )
$$;

CREATE OR REPLACE FUNCTION maintain_entry_stats()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    old_visible BOOLEAN := TG_OP <> 'INSERT' AND prts_entry_is_effectively_visible(OLD);
    new_visible BOOLEAN := TG_OP <> 'DELETE' AND prts_entry_is_effectively_visible(NEW);
BEGIN
    IF TG_OP = 'UPDATE'
       AND old_visible
       AND new_visible
       AND OLD.project_id = NEW.project_id
       AND OLD.file_id = NEW.file_id THEN
        IF OLD.state IS DISTINCT FROM NEW.state THEN
            UPDATE project_stats SET
                untranslated_count = untranslated_count
                    + CASE WHEN NEW.state = 'untranslated' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'untranslated' THEN 1 ELSE 0 END,
                translated_count = translated_count
                    + CASE WHEN NEW.state = 'translated' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'translated' THEN 1 ELSE 0 END,
                questioned_count = questioned_count
                    + CASE WHEN NEW.state = 'questioned' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'questioned' THEN 1 ELSE 0 END,
                checked_count = checked_count
                    + CASE WHEN NEW.state = 'checked' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'checked' THEN 1 ELSE 0 END,
                reviewed_count = reviewed_count
                    + CASE WHEN NEW.state = 'reviewed' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'reviewed' THEN 1 ELSE 0 END,
                updated_at = now()
            WHERE project_id = OLD.project_id;
            UPDATE file_stats SET
                untranslated_count = untranslated_count
                    + CASE WHEN NEW.state = 'untranslated' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'untranslated' THEN 1 ELSE 0 END,
                translated_count = translated_count
                    + CASE WHEN NEW.state = 'translated' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'translated' THEN 1 ELSE 0 END,
                questioned_count = questioned_count
                    + CASE WHEN NEW.state = 'questioned' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'questioned' THEN 1 ELSE 0 END,
                checked_count = checked_count
                    + CASE WHEN NEW.state = 'checked' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'checked' THEN 1 ELSE 0 END,
                reviewed_count = reviewed_count
                    + CASE WHEN NEW.state = 'reviewed' THEN 1 ELSE 0 END
                    - CASE WHEN OLD.state = 'reviewed' THEN 1 ELSE 0 END,
                updated_at = now()
            WHERE file_id = OLD.file_id;
        END IF;
        RETURN NEW;
    END IF;
    IF old_visible
       AND (
           TG_OP <> 'DELETE'
           OR EXISTS (
               SELECT 1 FROM file_stats
               WHERE file_id = OLD.file_id AND visible_total > 0
           )
       ) THEN
        PERFORM prts_apply_entry_stats_delta(OLD.project_id, OLD.file_id, OLD.state, -1);
    END IF;
    IF new_visible THEN
        PERFORM prts_apply_entry_stats_delta(NEW.project_id, NEW.file_id, NEW.state, 1);
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION maintain_project_stats_row()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO project_stats (project_id) VALUES (NEW.id)
    ON CONFLICT (project_id) DO NOTHING;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION maintain_file_stats_row()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO file_stats (file_id, project_id) VALUES (NEW.id, NEW.project_id)
    ON CONFLICT (file_id) DO NOTHING;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION subtract_file_stats_before_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE current_stats file_stats;
BEGIN
    SELECT * INTO current_stats FROM file_stats WHERE file_id = OLD.id;
    IF FOUND AND OLD.deleted_at IS NULL THEN
        UPDATE project_stats SET
            visible_total = visible_total - current_stats.visible_total,
            untranslated_count = untranslated_count - current_stats.untranslated_count,
            translated_count = translated_count - current_stats.translated_count,
            questioned_count = questioned_count - current_stats.questioned_count,
            checked_count = checked_count - current_stats.checked_count,
            reviewed_count = reviewed_count - current_stats.reviewed_count,
            updated_at = now()
        WHERE project_id = OLD.project_id;
        UPDATE file_stats SET
            visible_total = 0,
            untranslated_count = 0,
            translated_count = 0,
            questioned_count = 0,
            checked_count = 0,
            reviewed_count = 0,
            updated_at = now()
        WHERE file_id = OLD.id;
    END IF;
    RETURN OLD;
END;
$$;

INSERT INTO project_stats (
    project_id, visible_total, untranslated_count, translated_count,
    questioned_count, checked_count, reviewed_count
)
SELECT project.id,
       count(entry.id),
       count(entry.id) FILTER (WHERE entry.state = 'untranslated'),
       count(entry.id) FILTER (WHERE entry.state = 'translated'),
       count(entry.id) FILTER (WHERE entry.state = 'questioned'),
       count(entry.id) FILTER (WHERE entry.state = 'checked'),
       count(entry.id) FILTER (WHERE entry.state = 'reviewed')
FROM projects AS project
LEFT JOIN entries AS entry
  ON entry.project_id = project.id AND prts_entry_is_effectively_visible(entry)
GROUP BY project.id;

INSERT INTO file_stats (
    file_id, project_id, visible_total, untranslated_count, translated_count,
    questioned_count, checked_count, reviewed_count
)
SELECT file.id, file.project_id,
       count(entry.id),
       count(entry.id) FILTER (WHERE entry.state = 'untranslated'),
       count(entry.id) FILTER (WHERE entry.state = 'translated'),
       count(entry.id) FILTER (WHERE entry.state = 'questioned'),
       count(entry.id) FILTER (WHERE entry.state = 'checked'),
       count(entry.id) FILTER (WHERE entry.state = 'reviewed')
FROM files AS file
LEFT JOIN entries AS entry
  ON entry.file_id = file.id AND prts_entry_is_effectively_visible(entry)
GROUP BY file.id, file.project_id;

CREATE TRIGGER entries_stats_maintain_trg
    AFTER INSERT OR UPDATE OF file_id, project_id, state, hidden, deleted_at OR DELETE ON entries
    FOR EACH ROW EXECUTE FUNCTION maintain_entry_stats();

CREATE TRIGGER projects_stats_init_trg
    AFTER INSERT ON projects
    FOR EACH ROW EXECUTE FUNCTION maintain_project_stats_row();

CREATE TRIGGER files_stats_init_trg
    AFTER INSERT ON files
    FOR EACH ROW EXECUTE FUNCTION maintain_file_stats_row();

CREATE TRIGGER files_stats_delete_trg
    BEFORE DELETE ON files
    FOR EACH ROW EXECUTE FUNCTION subtract_file_stats_before_delete();

-- 每个 legacy 项目获得可恢复 repair job。worker 负责完整 BCP-47 canonicalization 与冲突隔离。
WITH repair_jobs AS (
    INSERT INTO jobs (kind, project_id, stage, payload, max_attempts)
    SELECT 'language_repair', id, 'projects', jsonb_build_object('project_id', id), 5
    FROM projects
    RETURNING id, project_id
)
UPDATE projects AS project
SET language_repair_job_id = repair_jobs.id
FROM repair_jobs
WHERE project.id = repair_jobs.project_id;

INSERT INTO jobs (kind, project_id, stage, payload, max_attempts)
VALUES ('language_repair', NULL, 'users', '{}', 5);

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON language_resolution_issues, project_stats, file_stats TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON SEQUENCE language_resolution_issues_id_seq TO %I',
        runtime_role
    );
END;
$$;

-- Editor collaboration release: project comment policy, entry comments, term versions,
-- and a cross-device history diff preference. All content tables remain project-bound.

ALTER TABLE projects
    ADD COLUMN comment_policy TEXT NOT NULL DEFAULT 'private',
    ADD CONSTRAINT projects_comment_policy_chk
        CHECK (comment_policy IN ('private', 'internal', 'public'));

ALTER TABLE users
    ADD COLUMN entry_diff_mode TEXT NOT NULL DEFAULT 'word_inline',
    ADD CONSTRAINT users_entry_diff_mode_chk
        CHECK (entry_diff_mode IN ('character_inline', 'word_inline', 'side_by_side'));

CREATE TABLE entry_comments (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id       BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    entry_id         BIGINT NOT NULL REFERENCES entries (id) ON DELETE CASCADE,
    author_id        BIGINT REFERENCES users (id) ON DELETE SET NULL,
    author_name      TEXT NOT NULL,
    author_avatar_url TEXT,
    content          TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at       TIMESTAMPTZ,
    deleted_by       BIGINT REFERENCES users (id) ON DELETE SET NULL,
    CONSTRAINT entry_comments_content_chk CHECK (
        (deleted_at IS NULL AND char_length(content) BETWEEN 1 AND 4000)
        OR (deleted_at IS NOT NULL AND content = '')
    )
);
CREATE INDEX entry_comments_entry_cursor_idx
    ON entry_comments (entry_id, id DESC);
CREATE INDEX entry_comments_project_idx
    ON entry_comments (project_id, id DESC);
CREATE TRIGGER entry_comments_set_updated_at
    BEFORE UPDATE ON entry_comments FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE terms
    ADD COLUMN version BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN deleted_by BIGINT REFERENCES users (id) ON DELETE SET NULL;

ALTER TABLE entry_versions
    ADD COLUMN editor_name TEXT,
    ADD COLUMN editor_avatar_url TEXT;

-- Editor pagination must remain exact even when owner/manager explicitly includes hidden entries.
-- Keep hidden state counts beside the existing effective-visible counters so the read path never
-- falls back to COUNT(entries).
ALTER TABLE project_stats
    ADD COLUMN hidden_total BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_untranslated_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_translated_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_questioned_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_checked_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_reviewed_count BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT project_stats_hidden_nonnegative_chk CHECK (
        hidden_total >= 0 AND hidden_untranslated_count >= 0
        AND hidden_translated_count >= 0 AND hidden_questioned_count >= 0
        AND hidden_checked_count >= 0 AND hidden_reviewed_count >= 0
    ),
    ADD CONSTRAINT project_stats_hidden_sum_chk CHECK (
        hidden_total = hidden_untranslated_count + hidden_translated_count
            + hidden_questioned_count + hidden_checked_count + hidden_reviewed_count
    );

ALTER TABLE file_stats
    ADD COLUMN hidden_total BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_untranslated_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_translated_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_questioned_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_checked_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN hidden_reviewed_count BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT file_stats_hidden_nonnegative_chk CHECK (
        hidden_total >= 0 AND hidden_untranslated_count >= 0
        AND hidden_translated_count >= 0 AND hidden_questioned_count >= 0
        AND hidden_checked_count >= 0 AND hidden_reviewed_count >= 0
    ),
    ADD CONSTRAINT file_stats_hidden_sum_chk CHECK (
        hidden_total = hidden_untranslated_count + hidden_translated_count
            + hidden_questioned_count + hidden_checked_count + hidden_reviewed_count
    );

UPDATE project_stats AS stats
SET hidden_total = counts.hidden_total,
    hidden_untranslated_count = counts.hidden_untranslated_count,
    hidden_translated_count = counts.hidden_translated_count,
    hidden_questioned_count = counts.hidden_questioned_count,
    hidden_checked_count = counts.hidden_checked_count,
    hidden_reviewed_count = counts.hidden_reviewed_count
FROM (
    SELECT project.id AS project_id,
           count(entry.id) AS hidden_total,
           count(entry.id) FILTER (WHERE entry.state = 'untranslated') AS hidden_untranslated_count,
           count(entry.id) FILTER (WHERE entry.state = 'translated') AS hidden_translated_count,
           count(entry.id) FILTER (WHERE entry.state = 'questioned') AS hidden_questioned_count,
           count(entry.id) FILTER (WHERE entry.state = 'checked') AS hidden_checked_count,
           count(entry.id) FILTER (WHERE entry.state = 'reviewed') AS hidden_reviewed_count
    FROM projects AS project
    LEFT JOIN entries AS entry
      ON entry.project_id = project.id AND entry.hidden
     AND prts_entry_effective_visible(entry.id, TRUE)
    GROUP BY project.id
) AS counts
WHERE counts.project_id = stats.project_id;

UPDATE file_stats AS stats
SET hidden_total = counts.hidden_total,
    hidden_untranslated_count = counts.hidden_untranslated_count,
    hidden_translated_count = counts.hidden_translated_count,
    hidden_questioned_count = counts.hidden_questioned_count,
    hidden_checked_count = counts.hidden_checked_count,
    hidden_reviewed_count = counts.hidden_reviewed_count
FROM (
    SELECT file.id AS file_id,
           count(entry.id) AS hidden_total,
           count(entry.id) FILTER (WHERE entry.state = 'untranslated') AS hidden_untranslated_count,
           count(entry.id) FILTER (WHERE entry.state = 'translated') AS hidden_translated_count,
           count(entry.id) FILTER (WHERE entry.state = 'questioned') AS hidden_questioned_count,
           count(entry.id) FILTER (WHERE entry.state = 'checked') AS hidden_checked_count,
           count(entry.id) FILTER (WHERE entry.state = 'reviewed') AS hidden_reviewed_count
    FROM files AS file
    LEFT JOIN entries AS entry
      ON entry.file_id = file.id AND entry.hidden AND entry.deleted_at IS NULL
    GROUP BY file.id
) AS counts
WHERE counts.file_id = stats.file_id;

CREATE FUNCTION prts_apply_entry_stats_delta_v2(
    target_project_id BIGINT,
    target_file_id BIGINT,
    state_name TEXT,
    amount BIGINT,
    hidden_entry BOOLEAN
)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE delta RECORD;
BEGIN
    SELECT * INTO delta FROM prts_stats_state_delta(state_name, amount);
    IF hidden_entry THEN
        UPDATE project_stats SET
            hidden_total = hidden_total + amount,
            hidden_untranslated_count = hidden_untranslated_count + delta.untranslated,
            hidden_translated_count = hidden_translated_count + delta.translated,
            hidden_questioned_count = hidden_questioned_count + delta.questioned,
            hidden_checked_count = hidden_checked_count + delta.checked,
            hidden_reviewed_count = hidden_reviewed_count + delta.reviewed,
            updated_at = now()
        WHERE project_id = target_project_id;
        UPDATE file_stats SET
            hidden_total = hidden_total + amount,
            hidden_untranslated_count = hidden_untranslated_count + delta.untranslated,
            hidden_translated_count = hidden_translated_count + delta.translated,
            hidden_questioned_count = hidden_questioned_count + delta.questioned,
            hidden_checked_count = hidden_checked_count + delta.checked,
            hidden_reviewed_count = hidden_reviewed_count + delta.reviewed,
            updated_at = now()
        WHERE file_id = target_file_id;
    ELSE
        PERFORM prts_apply_entry_stats_delta(target_project_id, target_file_id, state_name, amount);
    END IF;
END;
$$;

CREATE FUNCTION prts_entry_structurally_visible(entry_row entries)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
AS $$
    SELECT entry_row.deleted_at IS NULL
       AND EXISTS (
           SELECT 1 FROM files AS file
           WHERE file.id = entry_row.file_id AND file.deleted_at IS NULL
       )
       AND NOT EXISTS (
           SELECT 1
           FROM files AS file
           JOIN folders AS ancestor
             ON ancestor.project_id = file.project_id
            AND (file.path = ancestor.path
                 OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\')
           WHERE file.id = entry_row.file_id AND ancestor.deleted_at IS NOT NULL
       )
$$;

CREATE OR REPLACE FUNCTION maintain_entry_stats()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    old_active BOOLEAN := TG_OP <> 'INSERT' AND prts_entry_structurally_visible(OLD);
    new_active BOOLEAN := TG_OP <> 'DELETE' AND prts_entry_structurally_visible(NEW);
BEGIN
    IF TG_OP = 'UPDATE'
       AND old_active AND new_active
       AND OLD.project_id = NEW.project_id AND OLD.file_id = NEW.file_id
       AND OLD.hidden IS NOT DISTINCT FROM NEW.hidden THEN
        IF OLD.state IS DISTINCT FROM NEW.state THEN
            PERFORM prts_apply_entry_stats_delta_v2(
                OLD.project_id, OLD.file_id, OLD.state, -1, OLD.hidden
            );
            PERFORM prts_apply_entry_stats_delta_v2(
                NEW.project_id, NEW.file_id, NEW.state, 1, NEW.hidden
            );
        END IF;
        RETURN NEW;
    END IF;
    IF old_active
       AND (
           TG_OP <> 'DELETE'
           OR EXISTS (
               SELECT 1 FROM file_stats
               WHERE file_id = OLD.file_id
                 AND CASE WHEN OLD.hidden THEN hidden_total > 0 ELSE visible_total > 0 END
           )
       ) THEN
        PERFORM prts_apply_entry_stats_delta_v2(
            OLD.project_id, OLD.file_id, OLD.state, -1, OLD.hidden
        );
    END IF;
    IF new_active THEN
        PERFORM prts_apply_entry_stats_delta_v2(
            NEW.project_id, NEW.file_id, NEW.state, 1, NEW.hidden
        );
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
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
            hidden_total = hidden_total - current_stats.hidden_total,
            hidden_untranslated_count = hidden_untranslated_count
                - current_stats.hidden_untranslated_count,
            hidden_translated_count = hidden_translated_count
                - current_stats.hidden_translated_count,
            hidden_questioned_count = hidden_questioned_count
                - current_stats.hidden_questioned_count,
            hidden_checked_count = hidden_checked_count - current_stats.hidden_checked_count,
            hidden_reviewed_count = hidden_reviewed_count - current_stats.hidden_reviewed_count,
            updated_at = now()
        WHERE project_id = OLD.project_id;
        UPDATE file_stats SET
            visible_total = 0,
            untranslated_count = 0,
            translated_count = 0,
            questioned_count = 0,
            checked_count = 0,
            reviewed_count = 0,
            hidden_total = 0,
            hidden_untranslated_count = 0,
            hidden_translated_count = 0,
            hidden_questioned_count = 0,
            hidden_checked_count = 0,
            hidden_reviewed_count = 0,
            updated_at = now()
        WHERE file_id = OLD.id;
    END IF;
    RETURN OLD;
END;
$$;

-- Snapshot existing actor presentation so history remains readable after profile changes/deletion.
UPDATE entry_versions AS version
SET editor_name = actor.username,
    editor_avatar_url = actor.avatar_url
FROM users AS actor
WHERE actor.id = version.editor_id;

CREATE TABLE term_versions (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id       BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    term_id          BIGINT NOT NULL REFERENCES terms (id) ON DELETE CASCADE,
    version          BIGINT NOT NULL,
    kind             TEXT NOT NULL,
    source_lang      TEXT NOT NULL,
    source_text      TEXT NOT NULL,
    translation      TEXT NOT NULL,
    notes            TEXT NOT NULL,
    pos_id           BIGINT REFERENCES pos_presets (id) ON DELETE SET NULL,
    archived_at      TIMESTAMPTZ,
    deleted_at       TIMESTAMPTZ,
    editor_id        BIGINT REFERENCES users (id) ON DELETE SET NULL,
    editor_name      TEXT NOT NULL,
    editor_avatar_url TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (term_id, version)
);
CREATE INDEX term_versions_term_cursor_idx
    ON term_versions (term_id, version DESC, id DESC);
CREATE INDEX term_versions_project_idx
    ON term_versions (project_id, id DESC);

-- Existing terms start with an explicit baseline so every future rollback has a complete snapshot.
INSERT INTO term_versions (
    project_id, term_id, version, kind, source_lang, source_text, translation,
    notes, pos_id, archived_at, deleted_at, editor_id, editor_name,
    editor_avatar_url, created_at
)
SELECT term.project_id, term.id, 1, 'baseline', term.source_lang, term.source_text,
       term.translation, term.notes, term.pos_id, term.archived_at, NULL,
       term.updated_by,
       COALESCE(actor.username, 'system'), actor.avatar_url, term.updated_at
FROM terms AS term
LEFT JOIN users AS actor ON actor.id = term.updated_by;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NOT NULL AND runtime_role <> '' THEN
        EXECUTE format(
            'GRANT SELECT, INSERT, UPDATE, DELETE ON entry_comments, term_versions TO %I',
            runtime_role
        );
        EXECUTE format(
            'GRANT USAGE, SELECT ON SEQUENCE entry_comments_id_seq, term_versions_id_seq TO %I',
            runtime_role
        );
        EXECUTE format(
            'GRANT EXECUTE ON FUNCTION prts_apply_entry_stats_delta_v2(BIGINT, BIGINT, TEXT, BIGINT, BOOLEAN) TO %I',
            runtime_role
        );
        EXECUTE format(
            'GRANT EXECUTE ON FUNCTION prts_entry_structurally_visible(entries) TO %I',
            runtime_role
        );
    END IF;
END $$;

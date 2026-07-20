-- Editor quality-of-life foundation: questioned overlay, personal save preview,
-- encrypted AI endpoint metadata, terminology match modes and built-in POS presets.

ALTER TABLE users
    ADD COLUMN preview_translation_diff BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN ai_source_preference TEXT NOT NULL DEFAULT 'auto',
    ADD CONSTRAINT users_ai_source_preference_chk
        CHECK (ai_source_preference IN ('auto', 'personal', 'project'));

ALTER TABLE entries
    ADD COLUMN questioned BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX entries_questioned_visible_idx
    ON entries (project_id, id) WHERE questioned AND deleted_at IS NULL;

ALTER TABLE entry_versions
    ADD COLUMN questioned BOOLEAN;

-- Preserve the old questioned workflow rows without losing either their translatedness or flag.
UPDATE entries
SET questioned = TRUE,
    state = CASE WHEN translation = '' THEN 'untranslated' ELSE 'translated' END
WHERE state = 'questioned';

UPDATE entry_versions
SET questioned = TRUE,
    state = CASE WHEN COALESCE(translation, '') = '' THEN 'untranslated' ELSE 'translated' END
WHERE state = 'questioned';

ALTER TABLE entries DROP CONSTRAINT entries_state_chk;
ALTER TABLE entries ADD CONSTRAINT entries_state_chk
    CHECK (state IN ('untranslated', 'translated', 'checked', 'reviewed'));

-- questioned_count is intentionally retained as an overlay count. It is no longer part of the
-- mutually-exclusive workflow sum.
ALTER TABLE project_stats DROP CONSTRAINT project_stats_sum_chk;
ALTER TABLE project_stats ADD CONSTRAINT project_stats_sum_chk CHECK (
    visible_total = untranslated_count + translated_count + checked_count + reviewed_count
);
ALTER TABLE project_stats ADD CONSTRAINT project_stats_questioned_overlay_chk CHECK (
    questioned_count BETWEEN 0 AND visible_total
);
ALTER TABLE project_stats DROP CONSTRAINT project_stats_hidden_sum_chk;
ALTER TABLE project_stats ADD CONSTRAINT project_stats_hidden_sum_chk CHECK (
    hidden_total = hidden_untranslated_count + hidden_translated_count
        + hidden_checked_count + hidden_reviewed_count
);
ALTER TABLE project_stats ADD CONSTRAINT project_stats_hidden_questioned_overlay_chk CHECK (
    hidden_questioned_count BETWEEN 0 AND hidden_total
);
ALTER TABLE file_stats DROP CONSTRAINT file_stats_sum_chk;
ALTER TABLE file_stats ADD CONSTRAINT file_stats_sum_chk CHECK (
    visible_total = untranslated_count + translated_count + checked_count + reviewed_count
);
ALTER TABLE file_stats ADD CONSTRAINT file_stats_questioned_overlay_chk CHECK (
    questioned_count BETWEEN 0 AND visible_total
);
ALTER TABLE file_stats DROP CONSTRAINT file_stats_hidden_sum_chk;
ALTER TABLE file_stats ADD CONSTRAINT file_stats_hidden_sum_chk CHECK (
    hidden_total = hidden_untranslated_count + hidden_translated_count
        + hidden_checked_count + hidden_reviewed_count
);
ALTER TABLE file_stats ADD CONSTRAINT file_stats_hidden_questioned_overlay_chk CHECK (
    hidden_questioned_count BETWEEN 0 AND hidden_total
);

UPDATE project_stats AS stats
SET untranslated_count = counts.untranslated_count,
    translated_count = counts.translated_count,
    questioned_count = counts.questioned_count,
    checked_count = counts.checked_count,
    reviewed_count = counts.reviewed_count,
    hidden_untranslated_count = counts.hidden_untranslated_count,
    hidden_translated_count = counts.hidden_translated_count,
    hidden_questioned_count = counts.hidden_questioned_count,
    hidden_checked_count = counts.hidden_checked_count,
    hidden_reviewed_count = counts.hidden_reviewed_count,
    updated_at = now()
FROM (
    SELECT project.id AS project_id,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'untranslated') AS untranslated_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'translated') AS translated_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.questioned) AS questioned_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'checked') AS checked_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'reviewed') AS reviewed_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'untranslated') AS hidden_untranslated_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'translated') AS hidden_translated_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.questioned) AS hidden_questioned_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'checked') AS hidden_checked_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'reviewed') AS hidden_reviewed_count
    FROM projects AS project
    LEFT JOIN entries AS entry
      ON entry.project_id = project.id AND prts_entry_structurally_visible(entry)
    GROUP BY project.id
) AS counts
WHERE counts.project_id = stats.project_id;

UPDATE file_stats AS stats
SET untranslated_count = counts.untranslated_count,
    translated_count = counts.translated_count,
    questioned_count = counts.questioned_count,
    checked_count = counts.checked_count,
    reviewed_count = counts.reviewed_count,
    hidden_untranslated_count = counts.hidden_untranslated_count,
    hidden_translated_count = counts.hidden_translated_count,
    hidden_questioned_count = counts.hidden_questioned_count,
    hidden_checked_count = counts.hidden_checked_count,
    hidden_reviewed_count = counts.hidden_reviewed_count,
    updated_at = now()
FROM (
    SELECT file.id AS file_id,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'untranslated') AS untranslated_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'translated') AS translated_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.questioned) AS questioned_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'checked') AS checked_count,
           count(entry.id) FILTER (WHERE NOT entry.hidden AND entry.state = 'reviewed') AS reviewed_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'untranslated') AS hidden_untranslated_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'translated') AS hidden_translated_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.questioned) AS hidden_questioned_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'checked') AS hidden_checked_count,
           count(entry.id) FILTER (WHERE entry.hidden AND entry.state = 'reviewed') AS hidden_reviewed_count
    FROM files AS file
    LEFT JOIN entries AS entry
      ON entry.file_id = file.id AND entry.deleted_at IS NULL
    GROUP BY file.id
) AS counts
WHERE counts.file_id = stats.file_id;

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
        0::BIGINT,
        CASE WHEN state_name = 'checked' THEN amount ELSE 0 END,
        CASE WHEN state_name = 'reviewed' THEN amount ELSE 0 END
$$;

-- Move an active entry between workflow buckets without temporarily changing the total. This
-- keeps the questioned overlay constraint valid even when every visible/hidden entry is tagged.
CREATE OR REPLACE FUNCTION prts_move_entry_stats_state_v2(
    target_project_id BIGINT,
    target_file_id BIGINT,
    old_state_name TEXT,
    new_state_name TEXT,
    hidden_entry BOOLEAN
)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    old_delta RECORD;
    new_delta RECORD;
BEGIN
    SELECT * INTO old_delta FROM prts_stats_state_delta(old_state_name, -1);
    SELECT * INTO new_delta FROM prts_stats_state_delta(new_state_name, 1);
    IF hidden_entry THEN
        UPDATE project_stats SET
            hidden_untranslated_count = hidden_untranslated_count
                + old_delta.untranslated + new_delta.untranslated,
            hidden_translated_count = hidden_translated_count
                + old_delta.translated + new_delta.translated,
            hidden_checked_count = hidden_checked_count + old_delta.checked + new_delta.checked,
            hidden_reviewed_count = hidden_reviewed_count
                + old_delta.reviewed + new_delta.reviewed,
            updated_at = now()
        WHERE project_id = target_project_id;
        UPDATE file_stats SET
            hidden_untranslated_count = hidden_untranslated_count
                + old_delta.untranslated + new_delta.untranslated,
            hidden_translated_count = hidden_translated_count
                + old_delta.translated + new_delta.translated,
            hidden_checked_count = hidden_checked_count + old_delta.checked + new_delta.checked,
            hidden_reviewed_count = hidden_reviewed_count
                + old_delta.reviewed + new_delta.reviewed,
            updated_at = now()
        WHERE file_id = target_file_id;
    ELSE
        UPDATE project_stats SET
            untranslated_count = untranslated_count
                + old_delta.untranslated + new_delta.untranslated,
            translated_count = translated_count + old_delta.translated + new_delta.translated,
            checked_count = checked_count + old_delta.checked + new_delta.checked,
            reviewed_count = reviewed_count + old_delta.reviewed + new_delta.reviewed,
            updated_at = now()
        WHERE project_id = target_project_id;
        UPDATE file_stats SET
            untranslated_count = untranslated_count
                + old_delta.untranslated + new_delta.untranslated,
            translated_count = translated_count + old_delta.translated + new_delta.translated,
            checked_count = checked_count + old_delta.checked + new_delta.checked,
            reviewed_count = reviewed_count + old_delta.reviewed + new_delta.reviewed,
            updated_at = now()
        WHERE file_id = target_file_id;
    END IF;
END;
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
            PERFORM prts_move_entry_stats_state_v2(
                NEW.project_id, NEW.file_id, OLD.state, NEW.state, NEW.hidden
            );
        END IF;
        IF OLD.questioned IS DISTINCT FROM NEW.questioned THEN
            IF NEW.hidden THEN
                UPDATE project_stats SET hidden_questioned_count = hidden_questioned_count
                    + CASE WHEN NEW.questioned THEN 1 ELSE -1 END, updated_at = now()
                WHERE project_id = NEW.project_id;
                UPDATE file_stats SET hidden_questioned_count = hidden_questioned_count
                    + CASE WHEN NEW.questioned THEN 1 ELSE -1 END, updated_at = now()
                WHERE file_id = NEW.file_id;
            ELSE
                UPDATE project_stats SET questioned_count = questioned_count
                    + CASE WHEN NEW.questioned THEN 1 ELSE -1 END, updated_at = now()
                WHERE project_id = NEW.project_id;
                UPDATE file_stats SET questioned_count = questioned_count
                    + CASE WHEN NEW.questioned THEN 1 ELSE -1 END, updated_at = now()
                WHERE file_id = NEW.file_id;
            END IF;
        END IF;
        RETURN NEW;
    END IF;
    IF old_active THEN
        -- Remove the overlay first so the immediate CHECK never observes questioned > total.
        IF OLD.questioned THEN
            IF OLD.hidden THEN
                UPDATE project_stats SET hidden_questioned_count = hidden_questioned_count - 1, updated_at = now() WHERE project_id = OLD.project_id;
                UPDATE file_stats SET hidden_questioned_count = hidden_questioned_count - 1, updated_at = now() WHERE file_id = OLD.file_id;
            ELSE
                UPDATE project_stats SET questioned_count = questioned_count - 1, updated_at = now() WHERE project_id = OLD.project_id;
                UPDATE file_stats SET questioned_count = questioned_count - 1, updated_at = now() WHERE file_id = OLD.file_id;
            END IF;
        END IF;
        PERFORM prts_apply_entry_stats_delta_v2(OLD.project_id, OLD.file_id, OLD.state, -1, OLD.hidden);
    END IF;
    IF new_active THEN
        PERFORM prts_apply_entry_stats_delta_v2(NEW.project_id, NEW.file_id, NEW.state, 1, NEW.hidden);
        IF NEW.questioned THEN
            IF NEW.hidden THEN
                UPDATE project_stats SET hidden_questioned_count = hidden_questioned_count + 1, updated_at = now() WHERE project_id = NEW.project_id;
                UPDATE file_stats SET hidden_questioned_count = hidden_questioned_count + 1, updated_at = now() WHERE file_id = NEW.file_id;
            ELSE
                UPDATE project_stats SET questioned_count = questioned_count + 1, updated_at = now() WHERE project_id = NEW.project_id;
                UPDATE file_stats SET questioned_count = questioned_count + 1, updated_at = now() WHERE file_id = NEW.file_id;
            END IF;
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER entries_stats_maintain_trg ON entries;
CREATE TRIGGER entries_stats_maintain_trg
    AFTER INSERT OR UPDATE OF file_id, project_id, state, questioned, hidden, deleted_at OR DELETE
    ON entries FOR EACH ROW EXECUTE FUNCTION maintain_entry_stats();

ALTER TABLE terms
    ADD COLUMN match_mode TEXT NOT NULL DEFAULT 'exact',
    ADD CONSTRAINT terms_match_mode_chk CHECK (match_mode IN ('exact', 'placeholder', 'regex'));
ALTER TABLE term_versions ADD COLUMN match_mode TEXT NOT NULL DEFAULT 'exact';
ALTER TABLE term_versions ADD CONSTRAINT term_versions_match_mode_chk
    CHECK (match_mode IN ('exact', 'placeholder', 'regex'));
ALTER TABLE terms DROP CONSTRAINT terms_identity_unique;
ALTER TABLE terms ADD CONSTRAINT terms_identity_unique
    UNIQUE NULLS NOT DISTINCT (project_id, source_lang, source_text, pos_id, match_mode);

CREATE TABLE user_ai_settings (
    user_id             BIGINT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    base_url            TEXT NOT NULL,
    model               TEXT NOT NULL,
    api_key_ciphertext  BYTEA NOT NULL,
    api_key_nonce       BYTEA NOT NULL,
    api_key_hint        TEXT NOT NULL,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE project_ai_settings (
    project_id          BIGINT PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    base_url            TEXT NOT NULL,
    model               TEXT NOT NULL,
    api_key_ciphertext  BYTEA NOT NULL,
    api_key_nonce       BYTEA NOT NULL,
    api_key_hint        TEXT NOT NULL,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    updated_by          BIGINT REFERENCES users (id) ON DELETE SET NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO pos_presets (name_zh_cn, name_en, sort_order)
VALUES
    ('名词', 'Noun', 10), ('专有名词', 'Proper noun', 20), ('动词', 'Verb', 30),
    ('形容词', 'Adjective', 40), ('副词', 'Adverb', 50), ('代词', 'Pronoun', 60),
    ('数词', 'Numeral', 70), ('量词', 'Classifier', 80), ('介词', 'Preposition', 90),
    ('连词', 'Conjunction', 100), ('助词', 'Particle', 110),
    ('感叹词', 'Interjection', 120), ('短语', 'Phrase', 130), ('其他', 'Other', 140)
ON CONFLICT DO NOTHING;

UPDATE workspace_foundation_state SET schema_revision = 17, updated_at = now() WHERE singleton;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON user_ai_settings, project_ai_settings TO %I', runtime_role);
    EXECUTE format(
        'GRANT EXECUTE ON FUNCTION prts_move_entry_stats_state_v2(BIGINT, BIGINT, TEXT, TEXT, BOOLEAN) TO %I',
        runtime_role
    );
END;
$$;

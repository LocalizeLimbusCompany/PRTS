-- Primary-source search foundation。只能在 0008 的 repair gate 之后应用。

DROP TRIGGER entries_search_maintain_trg ON entries;

CREATE OR REPLACE FUNCTION entries_search_maintain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    source_language TEXT;
    target_language TEXT;
    repair_state TEXT;
    next_source TEXT;
BEGIN
    SELECT primary_source_lang, target_lang, language_repair_state
      INTO source_language, target_language, repair_state
      FROM projects
     WHERE id = NEW.project_id;

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

CREATE TRIGGER entries_search_maintain_trg
    BEFORE INSERT OR UPDATE ON entries
    FOR EACH ROW EXECUTE FUNCTION entries_search_maintain();

-- 只 reconcile 已完成 canonical repair 且 exact key 存在的行。
UPDATE entries AS entry
SET source_text = entry.original ->> project.primary_source_lang,
    source_tsv = to_tsvector(
        prts_ts_config(project.primary_source_lang),
        entry.original ->> project.primary_source_lang
    ),
    translation_tsv = to_tsvector(
        prts_ts_config(project.target_lang), COALESCE(entry.translation, '')
    ),
    embedding = NULL,
    embed_attempts = 0
FROM projects AS project
WHERE project.id = entry.project_id
  AND project.language_repair_state = 'ready'
  AND project.primary_source_lang IS NOT NULL
  AND entry.original ? project.primary_source_lang;

CREATE TABLE workspace_foundation_state (
    singleton                    BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    schema_revision              INTEGER NOT NULL,
    primary_search_revision      INTEGER NOT NULL,
    ready_project_count          BIGINT NOT NULL DEFAULT 0,
    unresolved_project_count     BIGINT NOT NULL DEFAULT 0,
    lexical_worker_registered    BOOLEAN NOT NULL DEFAULT FALSE,
    reconciled_at                TIMESTAMPTZ,
    updated_at                   TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO workspace_foundation_state (
    singleton, schema_revision, primary_search_revision,
    ready_project_count, unresolved_project_count, reconciled_at
)
SELECT TRUE, 8, 9,
       count(*) FILTER (WHERE language_repair_state = 'ready'),
       count(*) FILTER (WHERE language_repair_state = 'needs_language_resolution'),
       now()
FROM projects;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON workspace_foundation_state TO %I',
        runtime_role
    );
END;
$$;

-- 双语平台 POS 与 source-aware 项目术语。

CREATE TABLE pos_presets (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name_zh_cn  TEXT,
    name_en     TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pos_presets_name_chk CHECK (
        NULLIF(btrim(name_zh_cn), '') IS NOT NULL
        OR NULLIF(btrim(name_en), '') IS NOT NULL
    )
);
CREATE UNIQUE INDEX pos_presets_name_zh_cn_unique_idx
    ON pos_presets (lower(btrim(name_zh_cn)))
    WHERE NULLIF(btrim(name_zh_cn), '') IS NOT NULL;
CREATE UNIQUE INDEX pos_presets_name_en_unique_idx
    ON pos_presets (lower(btrim(name_en)))
    WHERE NULLIF(btrim(name_en), '') IS NOT NULL;
CREATE INDEX pos_presets_order_idx ON pos_presets (sort_order, id);
CREATE TRIGGER pos_presets_set_updated_at
    BEFORE UPDATE ON pos_presets FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE terms (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id   BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    source_lang  TEXT NOT NULL,
    source_text  TEXT NOT NULL,
    translation  TEXT NOT NULL DEFAULT '',
    notes        TEXT NOT NULL DEFAULT '',
    pos_id       BIGINT REFERENCES pos_presets (id) ON DELETE SET NULL,
    archived_at  TIMESTAMPTZ,
    created_by   BIGINT REFERENCES users (id) ON DELETE SET NULL,
    updated_by   BIGINT REFERENCES users (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT terms_source_lang_chk CHECK (btrim(source_lang) <> ''),
    CONSTRAINT terms_source_text_chk CHECK (btrim(source_text) <> ''),
    CONSTRAINT terms_identity_unique
        UNIQUE NULLS NOT DISTINCT (project_id, source_lang, source_text, pos_id)
);
CREATE INDEX terms_project_cursor_idx ON terms (project_id, id DESC);
CREATE INDEX terms_project_active_match_idx
    ON terms (project_id, source_lang, id) WHERE archived_at IS NULL;
CREATE INDEX terms_project_archived_cursor_idx
    ON terms (project_id, id DESC) WHERE archived_at IS NOT NULL;
CREATE TRIGGER terms_set_updated_at
    BEFORE UPDATE ON terms FOR EACH ROW EXECUTE FUNCTION set_updated_at();

UPDATE workspace_foundation_state
SET schema_revision = 12, updated_at = now()
WHERE singleton;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON pos_presets, terms TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON SEQUENCE pos_presets_id_seq, terms_id_seq TO %I',
        runtime_role
    );
END;
$$;

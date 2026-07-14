-- 编辑器与结构化搜索基础：删除词条 context，清理既有历史，并冻结 POST search schema。

-- 历史 payload 必须先于列删除完成清理；仅处理 entry allowlist，不触碰文件/文件夹结构项。
UPDATE file_change_items
SET before_value = CASE
        WHEN before_value IS NULL THEN NULL
        ELSE before_value - 'context'
    END,
    after_value = CASE
        WHEN after_value IS NULL THEN NULL
        ELSE after_value - 'context'
    END
WHERE entity_type = 'entry'
  AND (COALESCE(before_value, '{}'::JSONB) ? 'context'
       OR COALESCE(after_value, '{}'::JSONB) ? 'context');

ALTER TABLE entries DROP COLUMN context;

-- 结构化搜索 readiness 与 schema revision 由数据库明确声明，route 不靠迁移号猜测。
ALTER TABLE workspace_foundation_state
    ADD COLUMN editor_search_revision INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN structured_search_schema_ready BOOLEAN NOT NULL DEFAULT FALSE;

-- LIKE scope 使用统一转义，folder subtree 查询以 escaped_path || '/%' 配合 ESCAPE '\'。
CREATE FUNCTION prts_escape_like_pattern(value TEXT)
RETURNS TEXT
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
    SELECT replace(
        replace(
            replace(value, E'\\', E'\\\\'),
            '%', E'\\%'
        ),
        '_', E'\\_'
    )
$$;

-- recall 与 fetch 共用该 effective-visible 真值；include_hidden 只覆盖 hidden，绝不穿透
-- entry tombstone、deleted file 或任一 deleted ancestor folder。
CREATE FUNCTION prts_entry_effective_visible(
    target_entry_id BIGINT,
    include_hidden_entries BOOLEAN
)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
PARALLEL SAFE
AS $$
    WITH RECURSIVE scoped_entry AS (
        SELECT entry.deleted_at AS entry_deleted_at,
               entry.hidden,
               file.deleted_at AS file_deleted_at,
               file.folder_id
        FROM entries AS entry
        JOIN files AS file ON file.id = entry.file_id
        WHERE entry.id = target_entry_id
    ),
    folder_ancestors AS (
        SELECT folder.id, folder.parent_id, folder.deleted_at
        FROM folders AS folder
        JOIN scoped_entry ON scoped_entry.folder_id = folder.id
        UNION ALL
        SELECT parent.id, parent.parent_id, parent.deleted_at
        FROM folders AS parent
        JOIN folder_ancestors AS child ON child.parent_id = parent.id
    )
    SELECT COALESCE(
        (
            SELECT scoped_entry.entry_deleted_at IS NULL
               AND scoped_entry.file_deleted_at IS NULL
               AND (include_hidden_entries OR NOT scoped_entry.hidden)
               AND NOT EXISTS (
                   SELECT 1 FROM folder_ancestors
                   WHERE folder_ancestors.deleted_at IS NOT NULL
               )
            FROM scoped_entry
        ),
        FALSE
    )
$$;

-- 五种 scope 与稳定 entry-id continuation 所需的有界 active-row 索引。
CREATE INDEX entries_structured_search_active_idx
    ON entries (project_id, state, id)
    WHERE deleted_at IS NULL;

CREATE INDEX files_structured_search_active_path_idx
    ON files (project_id, path, id)
    WHERE deleted_at IS NULL;

CREATE INDEX folders_structured_search_active_path_idx
    ON folders (project_id, path, id)
    WHERE deleted_at IS NULL;

CREATE INDEX task_files_structured_search_active_idx
    ON task_files (task_id, live_file_id)
    WHERE live_file_id IS NOT NULL;

UPDATE workspace_foundation_state
SET schema_revision = GREATEST(schema_revision, 13),
    editor_search_revision = 13,
    structured_search_schema_ready = TRUE,
    updated_at = now()
WHERE singleton;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT EXECUTE ON FUNCTION prts_escape_like_pattern(TEXT) TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT EXECUTE ON FUNCTION prts_entry_effective_visible(BIGINT, BOOLEAN) TO %I',
        runtime_role
    );
END;
$$;

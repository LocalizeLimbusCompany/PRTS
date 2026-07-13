-- 项目任务、immutable baseline IDs、nullable live refs 与物化进度。

CREATE TABLE tasks (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id   BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    created_by   BIGINT REFERENCES users (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT tasks_title_chk CHECK (btrim(title) <> '')
);
CREATE INDEX tasks_project_cursor_idx ON tasks (project_id, id DESC);
CREATE TRIGGER tasks_set_updated_at
    BEFORE UPDATE ON tasks FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE task_files (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    task_id           BIGINT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    file_id_snapshot  BIGINT NOT NULL,
    live_file_id      BIGINT REFERENCES files (id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT task_files_snapshot_chk CHECK (file_id_snapshot > 0)
);
CREATE UNIQUE INDEX task_files_active_unique_idx
    ON task_files (task_id, live_file_id) WHERE live_file_id IS NOT NULL;
CREATE INDEX task_files_task_cursor_idx ON task_files (task_id, id);
CREATE INDEX task_files_live_file_idx ON task_files (live_file_id, task_id)
    WHERE live_file_id IS NOT NULL;

CREATE TABLE task_baseline_entries (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    task_file_id       BIGINT NOT NULL REFERENCES task_files (id) ON DELETE CASCADE,
    entry_id_snapshot  BIGINT NOT NULL,
    live_entry_id      BIGINT REFERENCES entries (id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT task_baseline_entries_snapshot_chk CHECK (entry_id_snapshot > 0),
    UNIQUE (task_file_id, entry_id_snapshot)
);
CREATE UNIQUE INDEX task_baseline_entries_live_unique_idx
    ON task_baseline_entries (task_file_id, live_entry_id)
    WHERE live_entry_id IS NOT NULL;
CREATE INDEX task_baseline_entries_live_entry_idx
    ON task_baseline_entries (live_entry_id, task_file_id)
    WHERE live_entry_id IS NOT NULL;

CREATE TABLE task_stats (
    task_id       BIGINT PRIMARY KEY REFERENCES tasks (id) ON DELETE CASCADE,
    denominator   BIGINT NOT NULL DEFAULT 0,
    completed     BIGINT NOT NULL DEFAULT 0,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT task_stats_nonnegative_chk CHECK (
        denominator >= 0 AND completed >= 0
    ),
    CONSTRAINT task_stats_completed_chk CHECK (completed <= denominator)
);

CREATE OR REPLACE FUNCTION maintain_task_stats_row()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO task_stats (task_id) VALUES (NEW.id)
    ON CONFLICT (task_id) DO NOTHING;
    RETURN NEW;
END;
$$;
CREATE TRIGGER tasks_stats_init_trg
    AFTER INSERT ON tasks
    FOR EACH ROW EXECUTE FUNCTION maintain_task_stats_row();

UPDATE workspace_foundation_state
SET schema_revision = 11, updated_at = now()
WHERE singleton;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON tasks, task_files, '
        'task_baseline_entries, task_stats TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON SEQUENCE tasks_id_seq, task_files_id_seq, '
        'task_baseline_entries_id_seq TO %I',
        runtime_role
    );
END;
$$;

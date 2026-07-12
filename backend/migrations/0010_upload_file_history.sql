-- 上传批次、文件历史与软删除生命周期的完整 schema。
-- 本迁移先验证 foundation 期间没有 writer 使用预建删除字段，避免半切换数据。

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM folders WHERE deletion_change_set_id IS NOT NULL)
       OR EXISTS (SELECT 1 FROM files WHERE deletion_change_set_id IS NOT NULL)
       OR EXISTS (SELECT 1 FROM entries WHERE deletion_change_set_id IS NOT NULL) THEN
        RAISE EXCEPTION '0010 requires all legacy deletion_change_set_id values to be NULL';
    END IF;
END;
$$;

CREATE TABLE file_change_sets (
    id              UUID PRIMARY KEY,
    project_id      BIGINT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    file_id         BIGINT REFERENCES files (id) ON DELETE SET NULL,
    folder_id       BIGINT REFERENCES folders (id) ON DELETE SET NULL,
    actor_id        BIGINT REFERENCES users (id) ON DELETE SET NULL,
    operation       TEXT NOT NULL,
    path_snapshot   TEXT NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT file_change_sets_operation_chk CHECK (
        operation IN ('upload_replace', 'move', 'rename', 'delete', 'restore', 'rollback')
    ),
    CONSTRAINT file_change_sets_target_chk CHECK (
        file_id IS NOT NULL OR folder_id IS NOT NULL OR operation = 'upload_replace'
    ),
    CONSTRAINT file_change_sets_metadata_chk CHECK (
        jsonb_typeof(metadata) = 'object' AND NOT prts_jsonb_contains_secret_key(metadata)
    )
);
CREATE INDEX file_change_sets_project_created_idx
    ON file_change_sets (project_id, created_at DESC, id);
CREATE INDEX file_change_sets_file_idx ON file_change_sets (file_id, created_at DESC)
    WHERE file_id IS NOT NULL;
CREATE INDEX file_change_sets_folder_idx ON file_change_sets (folder_id, created_at DESC)
    WHERE folder_id IS NOT NULL;

CREATE TABLE file_change_items (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    change_set_id       UUID NOT NULL REFERENCES file_change_sets (id) ON DELETE CASCADE,
    entity_type         TEXT NOT NULL,
    entity_id_snapshot  BIGINT,
    operation           TEXT NOT NULL,
    before_value        JSONB,
    after_value         JSONB,
    ordinal             INTEGER NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT file_change_items_entity_chk
        CHECK (entity_type IN ('folder', 'file', 'entry')),
    CONSTRAINT file_change_items_operation_chk
        CHECK (operation IN ('create', 'update', 'move', 'delete', 'restore', 'tombstone')),
    CONSTRAINT file_change_items_values_chk CHECK (
        (before_value IS NULL OR jsonb_typeof(before_value) = 'object')
        AND (after_value IS NULL OR jsonb_typeof(after_value) = 'object')
        AND (before_value IS NULL OR NOT prts_jsonb_contains_secret_key(before_value))
        AND (after_value IS NULL OR NOT prts_jsonb_contains_secret_key(after_value))
    ),
    UNIQUE (change_set_id, ordinal)
);
CREATE INDEX file_change_items_change_set_idx ON file_change_items (change_set_id, ordinal);

ALTER TABLE folders
    ADD COLUMN purge_after TIMESTAMPTZ,
    ADD CONSTRAINT folders_deletion_change_set_fk
        FOREIGN KEY (deletion_change_set_id) REFERENCES file_change_sets (id) ON DELETE RESTRICT,
    ADD CONSTRAINT folders_deletion_metadata_chk CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND purge_after IS NULL
            AND deletion_change_set_id IS NULL)
        OR (deleted_at IS NOT NULL AND purge_after IS NOT NULL
            AND deletion_change_set_id IS NOT NULL)
    );

ALTER TABLE files
    ADD COLUMN purge_after TIMESTAMPTZ,
    ADD CONSTRAINT files_deletion_change_set_fk
        FOREIGN KEY (deletion_change_set_id) REFERENCES file_change_sets (id) ON DELETE RESTRICT,
    ADD CONSTRAINT files_deletion_metadata_chk CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND purge_after IS NULL
            AND deletion_change_set_id IS NULL)
        OR (deleted_at IS NOT NULL AND purge_after IS NOT NULL
            AND deletion_change_set_id IS NOT NULL)
    );

ALTER TABLE entries
    ADD CONSTRAINT entries_deletion_change_set_fk
        FOREIGN KEY (deletion_change_set_id) REFERENCES file_change_sets (id) ON DELETE RESTRICT,
    ADD CONSTRAINT entries_deletion_metadata_chk CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND deletion_change_set_id IS NULL)
        OR (deleted_at IS NOT NULL AND deletion_change_set_id IS NOT NULL)
    );

ALTER TABLE folders DROP CONSTRAINT folders_project_id_path_key;
ALTER TABLE files DROP CONSTRAINT files_project_id_path_key;
DROP INDEX folders_project_active_path_idx;
DROP INDEX files_project_active_path_idx;
CREATE UNIQUE INDEX folders_project_active_path_idx
    ON folders (project_id, path) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX files_project_active_path_idx
    ON files (project_id, path) WHERE deleted_at IS NULL;

CREATE TABLE upload_batches (
    id                    BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id            BIGINT REFERENCES projects (id) ON DELETE SET NULL,
    project_id_snapshot   BIGINT NOT NULL,
    actor_id              BIGINT REFERENCES users (id) ON DELETE SET NULL,
    state                 TEXT NOT NULL DEFAULT 'draft',
    declared_file_count   INTEGER NOT NULL,
    declared_total_bytes  BIGINT NOT NULL,
    expires_at            TIMESTAMPTZ NOT NULL,
    completed_at          TIMESTAMPTZ,
    cancelled_at          TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT upload_batches_state_chk CHECK (
        state IN (
            'draft', 'uploading', 'queued', 'processing', 'cancelling', 'cancelled',
            'partially_succeeded', 'succeeded', 'failed', 'expired'
        )
    ),
    CONSTRAINT upload_batches_declared_chk CHECK (
        declared_file_count > 0 AND declared_total_bytes >= 0
    )
);
CREATE INDEX upload_batches_project_idx
    ON upload_batches (project_id_snapshot, id DESC);
CREATE INDEX upload_batches_expiry_idx
    ON upload_batches (expires_at, id)
    WHERE state IN ('draft', 'uploading', 'queued', 'processing', 'cancelling');
CREATE TRIGGER upload_batches_set_updated_at
    BEFORE UPDATE ON upload_batches FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE upload_batch_files (
    id                    BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    batch_id              BIGINT NOT NULL REFERENCES upload_batches (id) ON DELETE CASCADE,
    ordinal               INTEGER NOT NULL,
    path                  TEXT NOT NULL,
    declared_bytes        BIGINT NOT NULL,
    state                 TEXT NOT NULL DEFAULT 'uploading',
    current_attempt_id    BIGINT,
    processing_job_id     BIGINT REFERENCES jobs (id) ON DELETE SET NULL,
    target_file_id        BIGINT REFERENCES files (id) ON DELETE SET NULL,
    last_error_code       TEXT,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT upload_batch_files_state_chk CHECK (
        state IN ('uploading', 'queued', 'processing', 'succeeded', 'failed', 'cancelled', 'expired')
    ),
    CONSTRAINT upload_batch_files_declared_chk CHECK (declared_bytes >= 0),
    UNIQUE (batch_id, ordinal),
    UNIQUE (batch_id, path)
);
CREATE INDEX upload_batch_files_batch_idx ON upload_batch_files (batch_id, ordinal);
CREATE INDEX upload_batch_files_job_idx ON upload_batch_files (processing_job_id)
    WHERE processing_job_id IS NOT NULL;
CREATE TRIGGER upload_batch_files_set_updated_at
    BEFORE UPDATE ON upload_batch_files FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE upload_file_attempts (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    batch_file_id       BIGINT NOT NULL REFERENCES upload_batch_files (id) ON DELETE CASCADE,
    attempt_number      INTEGER NOT NULL,
    state               TEXT NOT NULL DEFAULT 'uploading',
    temp_key            TEXT NOT NULL UNIQUE,
    bytes_received      BIGINT NOT NULL DEFAULT 0,
    target_file_id      BIGINT REFERENCES files (id) ON DELETE SET NULL,
    error_code          TEXT,
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at         TIMESTAMPTZ,
    cleanup_after       TIMESTAMPTZ NOT NULL,
    cleaned_at          TIMESTAMPTZ,
    CONSTRAINT upload_file_attempts_state_chk CHECK (
        state IN (
            'uploading', 'receiving', 'queued', 'processing', 'succeeded',
            'failed', 'cancelled', 'expired'
        )
    ),
    CONSTRAINT upload_file_attempts_bytes_chk CHECK (bytes_received >= 0),
    UNIQUE (batch_file_id, attempt_number)
);
CREATE INDEX upload_file_attempts_file_idx
    ON upload_file_attempts (batch_file_id, attempt_number DESC);
CREATE INDEX upload_file_attempts_cleanup_idx
    ON upload_file_attempts (cleanup_after, id)
    WHERE cleaned_at IS NULL
      AND state IN ('failed', 'cancelled', 'expired', 'succeeded');

ALTER TABLE upload_batch_files
    ADD CONSTRAINT upload_batch_files_current_attempt_fk
        FOREIGN KEY (current_attempt_id) REFERENCES upload_file_attempts (id) ON DELETE SET NULL;

ALTER TABLE jobs
    ADD COLUMN upload_batch_file_id BIGINT REFERENCES upload_batch_files (id) ON DELETE SET NULL,
    ADD COLUMN target_file_id BIGINT REFERENCES files (id) ON DELETE SET NULL;
CREATE INDEX jobs_upload_batch_file_idx ON jobs (upload_batch_file_id, id)
    WHERE upload_batch_file_id IS NOT NULL;

ALTER TABLE workspace_foundation_state
    ADD COLUMN upload_history_schema_ready BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN file_history_writer_ready BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE workspace_foundation_state
SET schema_revision = 10, upload_history_schema_ready = TRUE, updated_at = now()
WHERE singleton;

DO $$
DECLARE runtime_role TEXT := current_setting('prts.runtime_role', true);
BEGIN
    IF runtime_role IS NULL OR btrim(runtime_role) = '' THEN
        RAISE EXCEPTION 'prts.runtime_role must be set by the migration runner';
    END IF;
    EXECUTE format(
        'GRANT SELECT, INSERT, UPDATE, DELETE ON file_change_sets, file_change_items, '
        'upload_batches, upload_batch_files, upload_file_attempts TO %I',
        runtime_role
    );
    EXECUTE format(
        'GRANT USAGE, SELECT ON SEQUENCE file_change_items_id_seq, upload_batches_id_seq, '
        'upload_batch_files_id_seq, upload_file_attempts_id_seq TO %I',
        runtime_role
    );
END;
$$;

-- P2：项目 / 文件夹 / 文件 / 词条 / 成员 / 词条历史。
-- 复用 0002 创建的 set_updated_at() 触发器函数。

-- 项目
CREATE TABLE projects (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    slug         TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    visibility   TEXT NOT NULL DEFAULT 'public',   -- public|private
    source_langs TEXT[] NOT NULL DEFAULT '{}',     -- BCP-47
    target_lang  TEXT NOT NULL,                    -- BCP-47
    owner_id     BIGINT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT projects_visibility_chk CHECK (visibility IN ('public', 'private'))
);
CREATE INDEX projects_owner_idx ON projects (owner_id);
CREATE INDEX projects_visibility_idx ON projects (visibility);
CREATE TRIGGER projects_set_updated_at
    BEFORE UPDATE ON projects FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 项目成员（项目级角色）
CREATE TABLE memberships (
    project_id BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role       TEXT NOT NULL,                       -- owner|manager|reviewer|translator
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id),
    CONSTRAINT memberships_role_chk CHECK (role IN ('owner', 'manager', 'reviewer', 'translator'))
);
CREATE INDEX memberships_user_idx ON memberships (user_id);

-- 文件夹（自引用树）
CREATE TABLE folders (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    parent_id  BIGINT REFERENCES folders (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    path       TEXT NOT NULL,                       -- 规范化全路径，如 a/b
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, path)
);
CREATE INDEX folders_project_idx ON folders (project_id);
CREATE INDEX folders_parent_idx ON folders (parent_id);

-- 文件
CREATE TABLE files (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id  BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    folder_id   BIGINT REFERENCES folders (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL,                      -- 全路径，如 a/b/c.json
    entry_count INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, path)
);
CREATE INDEX files_project_idx ON files (project_id);
CREATE INDEX files_folder_idx ON files (folder_id);
CREATE TRIGGER files_set_updated_at
    BEFORE UPDATE ON files FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 词条（最小翻译单位）
CREATE TABLE entries (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    file_id     BIGINT NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    -- 冗余 project_id：便于跨文件查询 / 搜索（P4），随插入由文件所属项目填充。
    project_id  BIGINT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    original    JSONB NOT NULL DEFAULT '{}',        -- {bcp47: 源文本}
    context     TEXT NOT NULL DEFAULT '',
    translation TEXT NOT NULL DEFAULT '',
    state       TEXT NOT NULL DEFAULT 'untranslated',
    locked      BOOLEAN NOT NULL DEFAULT FALSE,
    hidden      BOOLEAN NOT NULL DEFAULT FALSE,
    version     BIGINT NOT NULL DEFAULT 1,          -- 乐观锁
    updated_by  BIGINT REFERENCES users (id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (file_id, key),                          -- key 文件内唯一
    CONSTRAINT entries_state_chk
        CHECK (state IN ('untranslated', 'translated', 'questioned', 'checked', 'reviewed'))
);
-- 键集分页（文件内按 id 游标）+ 状态过滤
CREATE INDEX entries_file_id_idx ON entries (file_id, id);
CREATE INDEX entries_project_state_idx ON entries (project_id, state);
CREATE INDEX entries_file_state_idx ON entries (file_id, state);
CREATE INDEX entries_key_idx ON entries (project_id, key);
CREATE TRIGGER entries_set_updated_at
    BEFORE UPDATE ON entries FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 词条历史（每次变更一条快照，含差异来源）
CREATE TABLE entry_versions (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    entry_id    BIGINT NOT NULL REFERENCES entries (id) ON DELETE CASCADE,
    version     BIGINT NOT NULL,
    kind        TEXT NOT NULL,                      -- create|translate|edit|review|state|source_update
    translation TEXT,
    state       TEXT,
    original    JSONB,
    editor_id   BIGINT REFERENCES users (id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX entry_versions_entry_idx ON entry_versions (entry_id, version);

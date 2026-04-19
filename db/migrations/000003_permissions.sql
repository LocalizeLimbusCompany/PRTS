CREATE TABLE IF NOT EXISTS project_members (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_code TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (project_id, user_id)
);

CREATE TABLE IF NOT EXISTS permission_nodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS project_roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (project_id, code)
);

CREATE TABLE IF NOT EXISTS project_role_permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_role_id UUID NOT NULL REFERENCES project_roles(id) ON DELETE CASCADE,
    permission_node_id UUID NOT NULL REFERENCES permission_nodes(id) ON DELETE CASCADE,
    UNIQUE (project_role_id, permission_node_id)
);

CREATE TABLE IF NOT EXISTS project_member_permission_overrides (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission_node_id UUID NOT NULL REFERENCES permission_nodes(id) ON DELETE CASCADE,
    effect TEXT NOT NULL CHECK (effect IN ('allow', 'deny')),
    UNIQUE (project_id, user_id, permission_node_id)
);

CREATE TABLE IF NOT EXISTS project_member_document_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission_scope TEXT NOT NULL,
    match_type TEXT NOT NULL,
    match_value TEXT NOT NULL,
    effect TEXT NOT NULL CHECK (effect IN ('allow', 'deny')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_project_members_project_id ON project_members (project_id);
CREATE INDEX IF NOT EXISTS idx_project_roles_project_id ON project_roles (project_id);
CREATE INDEX IF NOT EXISTS idx_project_member_permission_overrides_project_user ON project_member_permission_overrides (project_id, user_id);
CREATE INDEX IF NOT EXISTS idx_project_member_document_rules_project_user ON project_member_document_rules (project_id, user_id);

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL DEFAULT '',
    display_name TEXT NOT NULL,
    preferred_locale TEXT NOT NULL DEFAULT 'zh-CN',
    preferred_source_language TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL DEFAULT 'public',
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    target_language TEXT NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'public',
    guest_policy TEXT NOT NULL DEFAULT 'read',
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (organization_id, slug)
);

CREATE TABLE IF NOT EXISTS project_source_languages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    language_code TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    UNIQUE (project_id, language_code)
);

CREATE TABLE IF NOT EXISTS document_tags (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#4C88FF',
    is_visible BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (project_id, code)
);

CREATE TABLE IF NOT EXISTS documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    current_version_no INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    UNIQUE (project_id, path)
);

CREATE TABLE IF NOT EXISTS document_tag_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    tag_id UUID NOT NULL REFERENCES document_tags(id) ON DELETE CASCADE,
    UNIQUE (document_id, tag_id)
);

CREATE TABLE IF NOT EXISTS document_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    version_no INTEGER NOT NULL,
    source_snapshot_hash TEXT NOT NULL DEFAULT '',
    import_job_id UUID,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (document_id, version_no)
);

CREATE TABLE IF NOT EXISTS translation_units (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    target_language TEXT NOT NULL,
    target_text TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'untranslated',
    comment TEXT NOT NULL DEFAULT '',
    version_no INTEGER NOT NULL DEFAULT 1,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (document_id, key)
);

CREATE TABLE IF NOT EXISTS translation_unit_sources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    translation_unit_id UUID NOT NULL REFERENCES translation_units(id) ON DELETE CASCADE,
    language_code TEXT NOT NULL,
    text TEXT NOT NULL DEFAULT '',
    UNIQUE (translation_unit_id, language_code)
);

CREATE TABLE IF NOT EXISTS translation_revisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    translation_unit_id UUID NOT NULL REFERENCES translation_units(id) ON DELETE CASCADE,
    revision_no INTEGER NOT NULL,
    before_target_text TEXT NOT NULL DEFAULT '',
    after_target_text TEXT NOT NULL DEFAULT '',
    before_status TEXT NOT NULL DEFAULT 'untranslated',
    after_status TEXT NOT NULL DEFAULT 'untranslated',
    change_note TEXT NOT NULL DEFAULT '',
    changed_by UUID REFERENCES users(id) ON DELETE SET NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (translation_unit_id, revision_no)
);

CREATE INDEX IF NOT EXISTS idx_projects_organization_id ON projects (organization_id);
CREATE INDEX IF NOT EXISTS idx_project_source_languages_project_id ON project_source_languages (project_id);
CREATE INDEX IF NOT EXISTS idx_document_tags_project_id ON document_tags (project_id);
CREATE INDEX IF NOT EXISTS idx_documents_project_id ON documents (project_id);
CREATE INDEX IF NOT EXISTS idx_document_versions_document_id ON document_versions (document_id);
CREATE INDEX IF NOT EXISTS idx_translation_units_project_id ON translation_units (project_id);
CREATE INDEX IF NOT EXISTS idx_translation_units_document_id ON translation_units (document_id);
CREATE INDEX IF NOT EXISTS idx_translation_units_status ON translation_units (status);
CREATE INDEX IF NOT EXISTS idx_translation_unit_sources_translation_unit_id ON translation_unit_sources (translation_unit_id);
CREATE INDEX IF NOT EXISTS idx_translation_revisions_translation_unit_id ON translation_revisions (translation_unit_id);

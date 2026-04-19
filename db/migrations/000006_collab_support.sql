CREATE TABLE IF NOT EXISTS glossary_terms (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_term TEXT NOT NULL,
    target_term TEXT NOT NULL,
    source_language TEXT NOT NULL,
    target_language TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS translation_memory_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_language TEXT NOT NULL,
    target_language TEXT NOT NULL,
    source_text TEXT NOT NULL,
    target_text TEXT NOT NULL,
    quality_score INTEGER NOT NULL DEFAULT 0,
    created_from_unit_id UUID REFERENCES translation_units(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS import_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    document_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    uploaded_by UUID REFERENCES users(id) ON DELETE SET NULL,
    error_message TEXT NOT NULL DEFAULT '',
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS export_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    file_path TEXT NOT NULL DEFAULT '',
    file_size BIGINT NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ,
    requested_by UUID REFERENCES users(id) ON DELETE SET NULL,
    error_message TEXT NOT NULL DEFAULT '',
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    detail_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_glossary_terms_project_id ON glossary_terms (project_id);
CREATE INDEX IF NOT EXISTS idx_translation_memory_entries_project_id ON translation_memory_entries (project_id);
CREATE INDEX IF NOT EXISTS idx_import_jobs_project_id ON import_jobs (project_id);
CREATE INDEX IF NOT EXISTS idx_export_jobs_project_id ON export_jobs (project_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_project_id ON audit_logs (project_id);

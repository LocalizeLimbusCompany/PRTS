ALTER TABLE translation_units
    ADD COLUMN IF NOT EXISTS is_questioned BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS is_locked BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS is_hidden BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_translation_units_is_questioned ON translation_units (is_questioned);
CREATE INDEX IF NOT EXISTS idx_translation_units_is_locked ON translation_units (is_locked);
CREATE INDEX IF NOT EXISTS idx_translation_units_is_hidden ON translation_units (is_hidden);

UPDATE users
SET preferred_source_language = 'ja'
WHERE preferred_source_language = 'jp';

UPDATE project_source_languages
SET language_code = 'ja'
WHERE language_code = 'jp';

UPDATE translation_unit_sources
SET language_code = 'ja'
WHERE language_code = 'jp';

UPDATE glossary_terms
SET source_language = 'ja'
WHERE source_language = 'jp';

UPDATE translation_memory_entries
SET source_language = 'ja'
WHERE source_language = 'jp';

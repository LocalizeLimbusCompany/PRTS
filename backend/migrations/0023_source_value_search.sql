-- Preserve individual source-language values for exact and boundary-sensitive search operators.
-- Ordinary contains/FTS paths continue to use source_all_text/source_all_tsv and their GIN indexes.
ALTER TABLE entries
    ADD COLUMN source_all_values TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    ADD COLUMN source_all_values_folded TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[];

CREATE OR REPLACE FUNCTION entries_source_values_maintain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    SELECT COALESCE(array_agg(value ORDER BY key), ARRAY[]::TEXT[]),
           COALESCE(array_agg(lower(value) ORDER BY key), ARRAY[]::TEXT[])
      INTO NEW.source_all_values, NEW.source_all_values_folded
      FROM jsonb_each_text(NEW.original);
    RETURN NEW;
END;
$$;

CREATE TRIGGER entries_source_values_maintain_trg
    BEFORE INSERT OR UPDATE OF original ON entries
    FOR EACH ROW EXECUTE FUNCTION entries_source_values_maintain();

UPDATE entries SET original = original;

CREATE INDEX entries_source_all_values_idx ON entries USING GIN (source_all_values);
CREATE INDEX entries_source_all_values_folded_idx ON entries USING GIN (source_all_values_folded);

UPDATE workspace_foundation_state
SET schema_revision = GREATEST(schema_revision, 23),
    updated_at = now()
WHERE singleton;

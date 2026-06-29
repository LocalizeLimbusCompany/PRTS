-- 0004_search.sql — 混合搜索：FTS(zhparser) + trgm + 向量列 + 触发器维护。

-- 1) 中文分词扩展与文本搜索配置
CREATE EXTENSION IF NOT EXISTS zhparser;
CREATE TEXT SEARCH CONFIGURATION prts_zh (PARSER = zhparser);
ALTER TEXT SEARCH CONFIGURATION prts_zh ADD MAPPING FOR n,v,a,i,e,l,j WITH simple;

-- 2) BCP-47 语言码 → regconfig（IMMUTABLE，供触发器复用）
CREATE FUNCTION prts_ts_config(lang text) RETURNS regconfig
LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE
    WHEN lang LIKE 'zh%' THEN 'prts_zh'::regconfig
    WHEN lang LIKE 'en%' THEN 'english'::regconfig
    ELSE 'simple'::regconfig
  END
$$;

-- 3) entries 增列（触发器维护）
ALTER TABLE entries
  ADD COLUMN source_text     TEXT NOT NULL DEFAULT '',
  ADD COLUMN source_tsv      tsvector,
  ADD COLUMN translation_tsv tsvector,
  ADD COLUMN embedding       vector(1024),
  ADD COLUMN embed_attempts  SMALLINT NOT NULL DEFAULT 0;

-- 4) 维护触发器：算 source_text/tsv；仅源文变化才作废 embedding
CREATE FUNCTION entries_search_maintain() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE src_lang text; tgt_lang text; new_src text;
BEGIN
  SELECT p.source_langs[1], p.target_lang INTO src_lang, tgt_lang
    FROM projects p WHERE p.id = NEW.project_id;
  new_src := COALESCE(NEW.original ->> src_lang, '');
  IF TG_OP = 'INSERT' OR new_src IS DISTINCT FROM OLD.source_text THEN
    NEW.embedding := NULL;
    NEW.embed_attempts := 0;
  END IF;
  NEW.source_text     := new_src;
  NEW.source_tsv      := to_tsvector(prts_ts_config(COALESCE(src_lang,'')), new_src);
  NEW.translation_tsv := to_tsvector(prts_ts_config(COALESCE(tgt_lang,'')), COALESCE(NEW.translation,''));
  RETURN NEW;
END $$;

CREATE TRIGGER entries_search_maintain_trg
  BEFORE INSERT OR UPDATE ON entries
  FOR EACH ROW EXECUTE FUNCTION entries_search_maintain();

-- 5) 索引
CREATE INDEX entries_source_tsv_idx       ON entries USING gin (source_tsv);
CREATE INDEX entries_translation_tsv_idx  ON entries USING gin (translation_tsv);
CREATE INDEX entries_source_trgm_idx      ON entries USING gin (source_text gin_trgm_ops);
CREATE INDEX entries_translation_trgm_idx ON entries USING gin (translation gin_trgm_ops);
CREATE INDEX entries_key_trgm_idx         ON entries USING gin (key gin_trgm_ops);
CREATE INDEX entries_embedding_hnsw_idx   ON entries USING hnsw (embedding vector_cosine_ops);

-- 6) 存量回填（embedding 保持 NULL → sweep worker 补）。
--    注：20w+ 大表生产环境应按 id 区间分批 UPDATE 以避免长事务/锁表。
UPDATE entries e SET
  source_text     = COALESCE(e.original ->> p.source_langs[1], ''),
  source_tsv      = to_tsvector(prts_ts_config(COALESCE(p.source_langs[1],'')), COALESCE(e.original ->> p.source_langs[1],'')),
  translation_tsv = to_tsvector(prts_ts_config(COALESCE(p.target_lang,'')), COALESCE(e.translation,''))
FROM projects p WHERE p.id = e.project_id;

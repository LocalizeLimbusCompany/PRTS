-- P0 初始迁移：启用搜索所需的 PostgreSQL 扩展。
-- pg_trgm：三元组模糊匹配（搜索容错/子串）。
-- vector ：pgvector，语义向量检索（见 plan §12、docs/architecture.md §3.3）。
--
-- 注意：vector 扩展需镜像预装 pgvector（见 deploy/docker-compose.yml 使用 pgvector/pgvector 镜像）。

CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS vector;

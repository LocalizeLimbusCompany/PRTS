<div align="center">

# PRTS

**Process-Review-Translation System** · An open, public L10N collaboration platform

[简体中文](./README.md) · [English](./README.en.md)

</div>

---

PRTS is a **public, extensible, high-concurrency** online translation platform for localization teams — an open-source counterpart to [Paratranz](https://paratranz.cn). Contributors **translate, review, and approve** text online, backed by role-based permissions, a complete audit history, hybrid search, and contribution points (CP).

> 🚧 The functional phases of the project-workspace overhaul are complete; final verification and release preparation are in progress. See [`plan/26-06-28-init_system.md`](./plan/26-06-28-init_system.md) and [`docs/architecture.md`](./docs/architecture.md) for the authoritative scope and verification boundaries.

## ✨ Features

- **Project workspace** with information, files, tasks, terminology, downloads, and management sections; the editor remains a separate full-screen route.
- **Project / Folder / File / Entry** hierarchy designed for the 200k+ entries target through materialized statistics, keyset pagination, and bounded batches.
- **Multiple source languages → one target language** (BCP-47, Hans/Hant aware), shown per user preference.
- **Real-time collaborative editor** (WebSocket): presence, "someone is editing" hints, optimistic-lock conflict guard.
- **Structured hybrid search**: POST tagged scopes, PostgreSQL full-text + trigram fuzzy + optional vector semantics (pgvector), RRF, and signed keyset cursors.
- **Durable streaming uploads**: 500-file / 2GB batch contract, 100MB per-file limit, byte-zero retries, per-file atomic replacement, cancellation/expiry cleanup, and 30-day recoverable history.
- **Permission-node RBAC**: platform roles (super admin / admin / maintainer) + project roles (owner / manager / reviewer / translator).
- **Contribution points and leaderboards**: online translations/edits award Levenshtein distance × 1.0, reviews/approvals × 0.3, with project all-time and platform all-time/UTC month/week rankings.
- **Pluggable auth providers**: password + OAuth2 (PKCE), with built-in ZOOT integration; supports OAuth-only mode.
- **History and audit**: business mutations and allowlisted redacted audit commit together and fail closed; file change sets support rollback/restore, while project deletion uses an owner-only challenge and a 24-hour delay.
- **Internationalization**: bilingual (zh-CN / en) frontend; backend localizes messages via `Accept-Language`.
- **Fully Dockerized**, with every API documented in Swagger.

## 🧱 Tech Stack

| Layer | Choice |
| --- | --- |
| Backend | Rust · tokio · **axum** · **sqlx** |
| Data | PostgreSQL (`pg_trgm` / `pgvector`) · Redis |
| Frontend | Vue 3 · Quasar 2 · Vite · pnpm · Pinia · vue-i18n |
| Docs | utoipa + Swagger UI |
| Deploy | Docker / docker-compose · GHCR · nginx |

## 🚀 Quick Start (Docker)

```bash
git clone git@github.com:LocalizeLimbusCompany/PRTS.git
cd PRTS
cp .env.example .env        # fill in DB / Redis / JWT / OAuth / Qwen, etc.
docker compose -f deploy/docker-compose.yml up -d
```

The migration account and application runtime account must be separate. On a fresh volume,
Compose creates the runtime role from `POSTGRES_MIGRATION_*` / `POSTGRES_RUNTIME_*` and runs
migrations in the one-shot `migrate` service; the backend receives only the runtime URL. When
upgrading an existing PostgreSQL volume, first have a database administrator create the matching
non-superuser runtime login role, then configure both URLs as shown in `.env.example`. Startup
fails closed if the roles are the same or the runtime role still owns tables. Keep real credentials
only in local environment variables and never commit them.

**An existing volume does not rerun PostgreSQL init scripts.** The database and table owner in an
older PRTS volume is usually `prts`; changing `.env` to the new default `prts_migrator` does not
create that role. Choose one upgrade path:

- Prefer reusing the real existing owner. Check `pg_database` / `pg_tables` first (the owner is
  usually `prts`), then point `POSTGRES_MIGRATION_USER`, `POSTGRES_MIGRATION_PASSWORD`, and
  `PRTS__DATABASE__MIGRATION_URL` at that **existing account which owns the old objects**. Keep the
  runtime URL on the newly created `prts_runtime` role only.
- If a dedicated `prts_migrator` is required, connect to the target database as the old owner or a
  database administrator, create only missing roles, set local passwords interactively with
  `psql`'s `\password`, and transfer all ownership. Replace `prts` below if the actual old owner is
  different:

  ```psql
  CREATE ROLE prts_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION;
  \password prts_migrator
  CREATE ROLE prts_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION;
  \password prts_runtime
  REASSIGN OWNED BY prts TO prts_migrator;
  ALTER DATABASE prts OWNER TO prts_migrator;
  ALTER SCHEMA public OWNER TO prts_migrator;
  ```

Only after that should the migration URL use `prts_migrator`. Never place real passwords in shell
history, either README, Compose files, or the repository; `\password` prompts interactively, and
the actual credentials belong only in the local `.env` / secret manager.

### Authentication session upgrade notice

This release rejects legacy access JWTs without a `sid` and makes PostgreSQL authoritative for
refresh sessions. Upgrading therefore invalidates all existing sessions, effectively signing
everyone out. Schedule a maintenance window and notify users before a production rollout.
Access is restored by signing in again after the upgrade; no account or password reset is needed.

Then:

- Frontend: `http://localhost:8080`
- API + Swagger: `http://localhost:3000/swagger-ui`

## 🛠️ Local Development

```bash
# Backend
cd backend
cargo run -p prts-api

# Frontend
cd frontend
pnpm install
pnpm dev
```

See [`docs/architecture.md`](./docs/architecture.md) and the in-crate / in-module docs for details.

### Verification

With PowerShell 7 installed, run from the repository root:

```powershell
pwsh -File scripts/verify-project-workspace.ps1
```

The default run performs static and automated contract checks only. Database and expensive scale verification require explicit switches plus PostgreSQL/Redis; do not report the 200k-entry or 100MB scenarios as measured without saved execution output.

During the compatibility window, the backend retains deprecated legacy inline-upload and GET-search endpoints. The new frontend uses upload batches and structured POST search only.

## 📁 Layout

```
backend/    Rust workspace (api / core / auth / search / realtime / db / common)
frontend/   Vue3 + Quasar frontend
docs/       Architecture & external integration docs
deploy/     Docker / compose / nginx
plan/       Planning documents
```

## 🤝 Contributing

Commits follow [Conventional Commits](https://www.conventionalcommits.org); code must pass `fmt` / `clippy` / lint and tests. Guidance for AI collaborators lives in [`CLAUDE.md`](./CLAUDE.md) and [`AGENTS.md`](./AGENTS.md).

## 📄 License

[MIT](./LICENSE)

---

Planned deployment domain: `prts.zeroasso.top`

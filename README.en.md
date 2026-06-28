<div align="center">

# PRTS

**Process-Review-Translation System** · An open, public L10N collaboration platform

[简体中文](./README.md) · [English](./README.en.md)

</div>

---

PRTS is a **public, extensible, high-concurrency** online translation platform for localization teams — an open-source counterpart to [Paratranz](https://paratranz.cn). Contributors **translate, review, and approve** text online, backed by role-based permissions, a complete audit history, a contribution score (CP), and hybrid search.

> 🚧 The project is in its bootstrap phase. See the plan in [`plan/26-06-28-init_system.md`](./plan/26-06-28-init_system.md) and the architecture in [`docs/architecture.md`](./docs/architecture.md).

## ✨ Features

- **Project / Folder / File / Entry** hierarchy; a single project scales to 200k+ entries.
- **Multiple source languages → one target language** (BCP-47, Hans/Hant aware), shown per user preference.
- **Real-time collaborative editor** (WebSocket): presence, "someone is editing" hints, optimistic-lock conflict guard.
- **Hybrid search**: PostgreSQL full-text + trigram fuzzy + vector semantics (pgvector), fused with RRF, with advanced filters.
- **Permission-node RBAC**: platform roles (super admin / admin / maintainer) + project roles (owner / manager / reviewer / translator).
- **Contribution points (CP)**: scored by edit distance.
- **Pluggable auth providers**: password + OAuth2 (PKCE), with built-in ZOOT integration; supports OAuth-only mode.
- **Full history & audit**: every action is recorded; admins can purge by time range / project.
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

TBD.

---

Planned deployment domain: `prts.zeroasso.top`

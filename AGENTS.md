# PRTS Translation System

## Product Scope

- This system is a public translation collaboration platform.
- One deployment must support multiple `Organizations`.
- One deployment must support multiple `Projects`.
- A project can support:
  - multiple source languages to one target language
  - one source language to one target language
  - arbitrary language combinations
- The initial use case is translating `en`, `jp`, and `kr` into `cn`, but the system must not hardcode these languages.
- Each `Project` represents exactly one translation line. If another source/target combination is needed, create another `Project`.
- The platform itself must support i18n.

## Current Non-Goals

- No machine translation for now.
- No Git repository integration for now.
- No cross-project translation memory.
- No cross-project glossary.
- No screenshot or visual context system for now.
- UI design is out of scope for Codex in this repository and will be handled by Gemini.

## Recommended Architecture

- Start with a modular monolith.
- Recommended stack:
  - Frontend: separate app, handled later
  - Backend API: Go
  - Database: PostgreSQL
  - Cache / async jobs: Redis
  - Object storage: optional, only if later needed for archives or attachments
- Split backend modules clearly even if they live in one service:
  - auth
  - organizations
  - projects
  - languages
  - translation documents
  - translation units
  - suggestions / submissions
  - review workflow
  - project glossary
  - project translation memory
  - versioning
  - import / export
  - audit log
- Recommended Go implementation style:
  - HTTP router: `chi` or `gin`
  - DB access: `pgx` + `sqlc`
  - background jobs: Go worker processes using Redis-backed queue or database-backed jobs
  - API shape: REST-first

## Core Domain Rules

- `Organization` owns multiple `Projects`.
- `Project` is the main translation isolation boundary.
- Translation memory and glossary are project-scoped.
- Documents can have tags.
- A tag can be shown in the frontend as a label, or be hidden depending on UI needs.
- Each `Project` has one configured target language and one configured source-language set.
- Each translation record should support multiple source-language texts plus one target-language text.
- Users can configure a preferred source language.
- The preferred source language should be emphasized in UI.
- Other source languages should still be available but collapsed by default in UI.
- The backend must expose all source-language variants for each translation unit.

## Suggested Data Model

- `User`
- `Organization`
- `OrganizationMember`
- `Project`
- `ProjectMember`
- `ProjectLanguageConfig`
- `Language`
- `DocumentTag`
- `Document`
- `DocumentVersion`
- `TranslationKey`
- `SourceText`
- `TargetText`
- `Suggestion`
- `Review`
- `GlossaryTerm`
- `TranslationMemoryEntry`
- `AuditLog`
- `ImportJob`
- `ExportJob`

## Versioning Requirements

- The platform itself must provide version management even without Git integration.
- Suggested versioning model:
  - every import creates a `DocumentVersion`
  - every accepted translation change is recorded in history
  - users can compare current content with a previous version
  - exports can target either the latest state or a selected version
- Do not implement versioning as plain overwrite-only storage.
- Keep versioning simple and practical, similar in spirit to MediaWiki history rather than Git branch/merge.
- v1 does not need multiple parallel translation drafts per unit.
- v1 stores one current target text plus history.

## File Format Rules

- Import format: JSON only.
- Export format: JSON only.
- Initial examples may use `en`, `jp`, `kr`, `cn`, but the schema must support arbitrary language codes.
- The import/export model should preserve language separation explicitly.
- Import is per-document JSON.
- Export returns multiple JSON documents for the whole project, one JSON per document.
- Typical document paths may look like `StoryData/S001B.json` or `StoryData/S002.json`.
- Internal storage does not need to stay JSON, but import/export must be JSON.
- Whole-project export may be delivered as a `zip` archive that contains multiple JSON files.
- Export artifacts should be cleaned up periodically.

## Suggested JSON Shape

```json
{
  "project": "example-project",
  "document": "ui/home.json",
  "entries": [
    {
      "key": "home.title",
      "source": {
        "en": "Rhodes Island Terminal",
        "jp": "ロドス・アイランド端末",
        "kr": "로도스 아일랜드 터미널"
      },
      "target": {
        "cn": "罗德岛终端"
      },
      "meta": {
        "status": "reviewed",
        "notes": ""
      }
    }
  ]
}
```

## Workflow Requirements

- Suggested states:
  - `untranslated`
  - `translated`
  - `reviewed`
  - `approved`
- Every change should be attributable to a user.
- Review actions should be auditable.

## Permissions

- Default project roles:
  - `owner`
  - `admin`
  - `reviewer`
  - `translator`
  - `guest`
- Baseline permissions:
  - `owner`: full control
  - `admin`: project administration and configurable guest access
  - `reviewer`: can translate and review
  - `translator`: can translate
  - `guest`: can view project content
- Permissions should be implemented as permission nodes rather than hardcoded role checks everywhere.
- Roles are bundles of permission nodes.
- The system should support:
  - role-based grants
  - user-specific permission overrides
  - file-level allowlists
  - file-level blocklists
- File tags can also be used later for filtering, display labels, and optional permission targeting.
- File-level permission scoping should support restricting translators or other users to only certain documents.
- Avoid baking all authorization directly into role names; keep roles as presets over a more granular permission model.

## Search and Filtering

- Must support at least:
  - filter by project
  - filter by document
  - filter by document tag
  - filter by key
  - filter by status
  - filter by source language text
  - filter by target language text

## API Design Guidance

- API-first.
- Keep frontend-specific presentation logic out of the backend.
- Return translation units in a structure that includes:
  - key
  - all source-language texts
  - target-language text
  - status
  - version metadata
  - audit metadata
- Backend contracts should be platform-i18n-ready.

## UI Handoff

- Frontend work should be handled by Gemini.
- Codex should not spend time on UI styling here.
- Gemini should use `UI/GEMINI_README.md` as the implementation handoff document.

## Confirmed Decisions

- The system includes `Organization`.
- Each `Project` supports exactly one translation line.
- Project uniqueness for translation units is `document + key`.
- Versioning should stay simple and practical, not Git-like.
- Import/export is document-oriented JSON, with project export as multiple JSON documents.
- Whole-project export may be transported as a `zip` archive.
- The platform itself must support i18n.
- Default project roles are `owner`, `admin`, `reviewer`, `translator`, and `guest`.
- Backend implementation should use Go.
- v1 keeps one current target text plus history instead of multiple active translation candidates.
- Authorization should support permission nodes, role bundles, user overrides, and file-level allow/block rules.

## Open Questions To Resolve Before Implementation

- The exact initial permission node list.
- Whether file-level allow/block rules should apply only to write actions or also to read actions.
- Whether export cleanup should be time-based only, count-based only, or both.

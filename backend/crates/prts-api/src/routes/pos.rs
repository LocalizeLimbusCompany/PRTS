//! 平台双语 POS 读取与 admin-only 管理。

use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::CurrentUser;
use crate::db_err;
use crate::error::{ApiError, ErrorResponse, RequestLocale};
use crate::state::AppState;
use crate::term_import::{
    self, DocumentFormat, ExportQuery, ImportConfirmDto, ImportConfirmRequest, ImportKind,
    ImportPreviewRequest, PosDocumentRow, PosImportPreviewDto, PosPreviewRowDto,
    ResolvedPosImportRow, StoredPreview,
};

/// 双语 POS 及按请求语言回退后的展示名。
#[derive(Debug, Serialize, ToSchema)]
pub struct PosDto {
    pub id: i64,
    pub name_zh_cn: Option<String>,
    pub name_en: Option<String>,
    pub display_name: String,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建或更新双语 POS；至少一个名称非空。
#[derive(Debug, Deserialize, ToSchema)]
pub struct PosWriteRequest {
    pub name_zh_cn: Option<String>,
    pub name_en: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
}

/// 列出全局 POS；展示名按 Accept-Language 在 zh-CN/en 间回退。
#[utoipa::path(get, path = "/pos", tag = "pos",
    description = "按 sort_order/id 列出全局双语词性预设。display_name 根据 Accept-Language 选择中文或英文，并在缺失时回退。",
    responses((status = 200, body = Vec<PosDto>), (status = 500, body = ErrorResponse)))]
pub async fn list_pos(
    State(state): State<AppState>,
    RequestLocale(locale): RequestLocale,
) -> Result<Json<Vec<PosDto>>, ApiError> {
    let rows = prts_db::pos::list(&state.db).await.map_err(db_err)?;
    Ok(Json(
        rows.into_iter().map(|row| pos_dto(row, locale)).collect(),
    ))
}

/// platform admin/super-admin 创建 POS；maintainer 与项目角色均无权。
#[utoipa::path(post, path = "/admin/pos", tag = "pos", request_body = PosWriteRequest,
    description = "创建全局双语词性预设。事务内重新校验当前平台角色，仅 platform admin/super-admin 可执行，并写不含名称正文的审计。",
    responses((status = 201, body = PosDto), (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
        (status = 409, body = ErrorResponse), (status = 503, body = ErrorResponse)))]
pub async fn create_pos(
    State(state): State<AppState>,
    user: CurrentUser,
    RequestLocale(locale): RequestLocale,
    Json(request): Json<PosWriteRequest>,
) -> Result<(StatusCode, Json<PosDto>), ApiError> {
    let (name_zh_cn, name_en) = normalize_names(&request)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_platform_pos_manage(&mut tx, user.id).await?;
    let pos = prts_db::pos::create_tx(
        &mut tx,
        name_zh_cn.as_deref(),
        name_en.as_deref(),
        request.sort_order,
    )
    .await
    .map_err(map_pos_db_error)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::PosCreated {
            pos_id: pos.id,
            has_zh_cn_name: pos.name_zh_cn.is_some(),
            has_en_name: pos.name_en.is_some(),
            sort_order: pos.sort_order,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(pos_dto(pos, locale))))
}

/// platform admin/super-admin 更新 POS；审计不包含双语名称正文。
#[utoipa::path(put, path = "/admin/pos/{pos_id}", tag = "pos",
    params(("pos_id" = i64, Path)), request_body = PosWriteRequest,
    description = "更新全局双语词性预设。事务内重新校验平台管理员权限并锁定 POS；项目 owner/manager 权限不会提升为平台权限。",
    responses((status = 200, body = PosDto), (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn update_pos(
    State(state): State<AppState>,
    user: CurrentUser,
    RequestLocale(locale): RequestLocale,
    Path(pos_id): Path<i64>,
    Json(request): Json<PosWriteRequest>,
) -> Result<Json<PosDto>, ApiError> {
    let (name_zh_cn, name_en) = normalize_names(&request)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_platform_pos_manage(&mut tx, user.id).await?;
    let current = prts_db::pos::find_for_update_tx(&mut tx, pos_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let mut changed_fields = Vec::new();
    if current.name_zh_cn != name_zh_cn {
        changed_fields.push("name_zh_cn");
    }
    if current.name_en != name_en {
        changed_fields.push("name_en");
    }
    if current.sort_order != request.sort_order {
        changed_fields.push("sort_order");
    }
    let pos = prts_db::pos::update_tx(
        &mut tx,
        pos_id,
        name_zh_cn.as_deref(),
        name_en.as_deref(),
        request.sort_order,
    )
    .await
    .map_err(map_pos_db_error)?
    .ok_or(prts_common::Error::NotFound)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::PosUpdated {
            pos_id,
            has_zh_cn_name: pos.name_zh_cn.is_some(),
            has_en_name: pos.name_en.is_some(),
            sort_order: pos.sort_order,
            changed_field_count: changed_fields.len(),
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(pos_dto(pos, locale)))
}

/// platform admin/super-admin 删除 POS；既有 term 的 pos_id 由显式 SET NULL 清除。
#[utoipa::path(delete, path = "/admin/pos/{pos_id}", tag = "pos",
    params(("pos_id" = i64, Path)),
    description = "删除全局词性预设；术语通过声明的 ON DELETE SET NULL 保留。事务内重新校验平台管理员权限并写脱敏审计。",
    responses((status = 204), (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn delete_pos(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(pos_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_platform_pos_manage(&mut tx, user.id).await?;
    prts_db::pos::find_for_update_tx(&mut tx, pos_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let affected_term_count = prts_db::pos::count_references_tx(&mut tx, pos_id)
        .await
        .map_err(db_err)?;
    if !prts_db::pos::delete_tx(&mut tx, pos_id)
        .await
        .map_err(map_pos_delete_error)?
    {
        return Err(prts_common::Error::NotFound.into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::PosDeleted {
            pos_id,
            affected_term_count,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 平台管理员解析并预览双语 POS CSV/JSON；不写 POS 业务表。
#[utoipa::path(post, path = "/admin/pos/imports/preview", tag = "pos",
    request_body = ImportPreviewRequest,
    description = "仅 platform admin/super-admin 可预览双语 POS CSV/JSON。名称先 trim 并按各语言大小写无关唯一性校验，再返回 created/updated 计划与绑定 actor/kind/digest 的十五分钟一次性 token；preview 不写 POS 表。",
    responses((status = 200, body = PosImportPreviewDto),
        (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse),
        (status = 500, body = ErrorResponse)))]
pub async fn preview_pos_import(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(request): Json<ImportPreviewRequest>,
) -> Result<Json<PosImportPreviewDto>, ApiError> {
    require_platform_pos_manage(&user)?;
    let rows = term_import::parse_pos_document(request.format, &request.content)
        .map_err(map_import_rule_error)?;
    let presets = prts_db::pos::list(&state.db).await.map_err(db_err)?;
    let resolved = resolve_pos_import_rows(&rows, &presets)?;
    let created = resolved.iter().filter(|row| row.id.is_none()).count();
    let updated = resolved.len() - created;
    let preview_rows = resolved
        .iter()
        .map(|row| PosPreviewRowDto {
            row: row.row,
            name_zh_cn: row.name_zh_cn.clone(),
            name_en: row.name_en.clone(),
            sort_order: row.sort_order,
            action: if row.id.is_some() {
                "updated".to_string()
            } else {
                "created".to_string()
            },
        })
        .collect();
    let digest = term_import::canonical_digest(&rows).map_err(map_import_rule_error)?;
    let token = term_import::store_preview(
        &state.cache,
        &StoredPreview {
            actor_id: user.id.to_string(),
            project_id: None,
            kind: ImportKind::Pos,
            digest: digest.clone(),
            primary_source_lang: None,
            terms: Vec::new(),
            pos: resolved,
        },
    )
    .await
    .map_err(map_preview_store_error)?;
    Ok(Json(PosImportPreviewDto {
        token,
        digest,
        expires_in_seconds: term_import::PREVIEW_TTL_SECONDS,
        created,
        updated,
        rows: preview_rows,
        warnings: Vec::new(),
    }))
}

/// 原子消费 POS preview，并在同一事务重查平台权限、批量计划与审计。
#[utoipa::path(post, path = "/admin/pos/imports/{token}/confirm", tag = "pos",
    params(("token" = String, Path)), request_body = ImportConfirmRequest,
    description = "使用 Redis Lua 原子校验 actor/kind/canonical digest 并一次性消费 POS preview，再在事务内重新读取用户平台权限、串行化 POS identity 计划、执行 create/update 并提交不含名称正文的 allowlisted audit。",
    responses((status = 200, body = ImportConfirmDto),
        (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
        (status = 409, body = ErrorResponse), (status = 500, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn confirm_pos_import(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(token): Path<String>,
    Json(request): Json<ImportConfirmRequest>,
) -> Result<Json<ImportConfirmDto>, ApiError> {
    let stored = term_import::take_bound_preview(
        &state.cache,
        &token,
        user.id,
        None,
        ImportKind::Pos,
        &request.digest,
    )
    .await
    .map_err(map_preview_store_error)?
    .ok_or(prts_common::Error::ImportPreviewTokenInvalid)?;
    let rows = stored
        .pos
        .iter()
        .map(|row| PosDocumentRow {
            name_zh_cn: row.name_zh_cn.clone(),
            name_en: row.name_en.clone(),
            sort_order: row.sort_order,
        })
        .collect::<Vec<_>>();
    let actual_digest = term_import::canonical_digest(&rows).map_err(map_import_rule_error)?;
    if actual_digest != stored.digest {
        return Err(prts_common::Error::ImportPreviewTokenInvalid.into());
    }

    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_platform_pos_manage(&mut tx, user.id).await?;
    let presets = prts_db::pos::list_for_import_tx(&mut tx)
        .await
        .map_err(db_err)?;
    let resolved = resolve_pos_import_rows(&rows, &presets)?;
    let created = resolved.iter().filter(|row| row.id.is_none()).count();
    let updated = resolved.len() - created;
    for row in &resolved {
        match row.id {
            Some(id) => {
                prts_db::pos::update_tx(
                    &mut tx,
                    id,
                    row.name_zh_cn.as_deref(),
                    row.name_en.as_deref(),
                    row.sort_order,
                )
                .await
                .map_err(map_pos_db_error)?
                .ok_or(prts_common::Error::NotFound)?;
            }
            None => {
                prts_db::pos::create_tx(
                    &mut tx,
                    row.name_zh_cn.as_deref(),
                    row.name_en.as_deref(),
                    row.sort_order,
                )
                .await
                .map_err(map_pos_db_error)?;
            }
        }
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::PosImported { created, updated },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(ImportConfirmDto {
        created,
        updated,
        warnings: Vec::new(),
    }))
}

/// 平台管理员导出稳定双语 POS CSV/JSON，并在返回正文前提交脱敏审计。
#[utoipa::path(get, path = "/admin/pos/export", tag = "pos", params(ExportQuery),
    description = "仅 platform admin/super-admin 可导出全部 POS。CSV/JSON 固定包含 name_zh_cn、name_en、sort_order；权限在审计事务内重新校验，审计只记录格式与行数。",
    responses((status = 200, description = "稳定 CSV/JSON POS 文档"),
        (status = 400, body = ErrorResponse), (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn export_pos(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let format = query.format.unwrap_or(DocumentFormat::Csv);
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_platform_pos_manage(&mut tx, user.id).await?;
    let presets = prts_db::pos::list_for_import_tx(&mut tx)
        .await
        .map_err(db_err)?;
    let rows = presets
        .iter()
        .map(|preset| PosDocumentRow {
            name_zh_cn: preset.name_zh_cn.clone(),
            name_en: preset.name_en.clone(),
            sort_order: preset.sort_order,
        })
        .collect::<Vec<_>>();
    let body = term_import::encode_pos_document(format, &rows)
        .map_err(|_| prts_common::Error::internal("POS export encoding failed"))?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::PosExported {
            row_count: rows.len(),
            format: format_name(format),
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    let (content_type, extension) = match format {
        DocumentFormat::Csv => ("text/csv; charset=utf-8", "csv"),
        DocumentFormat::Json => ("application/json; charset=utf-8", "json"),
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"pos-presets.{extension}\""),
            ),
        ],
        body,
    )
        .into_response())
}

fn resolve_pos_import_rows(
    rows: &[PosDocumentRow],
    presets: &[prts_db::models::PosPreset],
) -> Result<Vec<ResolvedPosImportRow>, ApiError> {
    let mut resolved = Vec::with_capacity(rows.len());
    let mut claimed_ids = HashSet::new();
    for (index, row) in rows.iter().enumerate() {
        let mut matches = HashSet::new();
        for preset in presets {
            if same_optional_name(row.name_zh_cn.as_deref(), preset.name_zh_cn.as_deref())
                || same_optional_name(row.name_en.as_deref(), preset.name_en.as_deref())
            {
                matches.insert(preset.id);
            }
        }
        if matches.len() > 1 {
            return Err(prts_common::Error::ImportPosAmbiguous.into());
        }
        let id = matches.into_iter().next();
        if id.is_some_and(|id| !claimed_ids.insert(id)) {
            return Err(prts_common::Error::ImportDuplicateRow.into());
        }
        resolved.push(ResolvedPosImportRow {
            row: index + 1,
            id,
            name_zh_cn: row.name_zh_cn.clone(),
            name_en: row.name_en.clone(),
            sort_order: row.sort_order,
        });
    }
    Ok(resolved)
}

fn same_optional_name(left: Option<&str>, right: Option<&str>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left.trim().to_lowercase() == right.trim().to_lowercase())
}

fn require_platform_pos_manage(user: &CurrentUser) -> Result<(), ApiError> {
    if user
        .platform_role
        .is_some_and(|role| role.has(nodes::PLATFORM_POS_MANAGE))
    {
        Ok(())
    } else {
        Err(prts_common::Error::Forbidden.into())
    }
}

fn map_import_rule_error(error: term_import::ImportRuleError) -> ApiError {
    match error {
        term_import::ImportRuleError::InvalidLanguageTag { .. } => {
            prts_common::Error::InvalidLanguageTag.into()
        }
        term_import::ImportRuleError::DuplicateRow { .. } => {
            prts_common::Error::ImportDuplicateRow.into()
        }
        term_import::ImportRuleError::ActiveSourceMismatch { .. } => {
            prts_common::Error::TermActiveSourceMismatch.into()
        }
        term_import::ImportRuleError::PosNameRequired { .. } => {
            prts_common::Error::PosNameRequired.into()
        }
        term_import::ImportRuleError::InvalidFormat
        | term_import::ImportRuleError::SourceTextRequired { .. } => {
            prts_common::Error::ImportFormatInvalid.into()
        }
    }
}

fn map_preview_store_error(error: term_import::PreviewStoreError) -> ApiError {
    match error {
        term_import::PreviewStoreError::InvalidPayload => {
            prts_common::Error::ImportPreviewTokenInvalid.into()
        }
        term_import::PreviewStoreError::Unavailable => {
            prts_common::Error::internal("terminology preview store unavailable").into()
        }
    }
}

fn format_name(format: DocumentFormat) -> &'static str {
    match format {
        DocumentFormat::Csv => "csv",
        DocumentFormat::Json => "json",
    }
}

async fn lock_platform_pos_manage(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
) -> Result<(), ApiError> {
    let user = prts_db::users::find_by_id_for_update_tx(conn, user_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::Unauthorized)?;
    let can_manage = user
        .platform_role
        .as_deref()
        .and_then(prts_core::PlatformRole::parse)
        .is_some_and(|role| role.has(nodes::PLATFORM_POS_MANAGE));
    if can_manage {
        Ok(())
    } else {
        Err(prts_common::Error::Forbidden.into())
    }
}

fn normalize_names(
    request: &PosWriteRequest,
) -> Result<(Option<String>, Option<String>), ApiError> {
    let normalize = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    };
    let names = (normalize(&request.name_zh_cn), normalize(&request.name_en));
    if names.0.is_none() && names.1.is_none() {
        Err(prts_common::Error::PosNameRequired.into())
    } else {
        Ok(names)
    }
}

fn map_pos_db_error(error: sqlx::Error) -> ApiError {
    let constraint = error
        .as_database_error()
        .and_then(|database| database.constraint());
    if matches!(
        constraint,
        Some("pos_presets_name_zh_cn_unique_idx" | "pos_presets_name_en_unique_idx")
    ) {
        prts_common::Error::DuplicatePosName.into()
    } else if constraint == Some("pos_presets_name_chk") {
        prts_common::Error::PosNameRequired.into()
    } else {
        db_err(error)
    }
}

fn map_pos_delete_error(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .and_then(|database| database.constraint())
        == Some("terms_identity_unique")
    {
        prts_common::Error::PosInUse.into()
    } else {
        db_err(error)
    }
}

fn audit_actor(user: &CurrentUser) -> AuditActor<'_> {
    AuditActor {
        id: Some(user.id),
        kind: AuditActorKind::User,
        ip: None,
    }
}

fn pos_dto(pos: prts_db::models::PosPreset, locale: prts_common::i18n::Locale) -> PosDto {
    let display_name = match locale {
        prts_common::i18n::Locale::ZhCn => pos.name_zh_cn.as_ref().or(pos.name_en.as_ref()),
        prts_common::i18n::Locale::En => pos.name_en.as_ref().or(pos.name_zh_cn.as_ref()),
    }
    .cloned()
    .unwrap_or_default();
    PosDto {
        id: pos.id,
        name_zh_cn: pos.name_zh_cn,
        name_en: pos.name_en,
        display_name,
        sort_order: pos.sort_order,
        created_at: pos.created_at.to_rfc3339(),
        updated_at: pos.updated_at.to_rfc3339(),
    }
}

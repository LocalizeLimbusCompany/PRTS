//! 项目结构化搜索：POST 主接口与 deprecated GET 兼容适配。
//!
//! handler 只做协议、项目可见性、scope 资源绑定、cursor 签名和 embedding 降级编排；
//! canonical conditions/scope/states/fingerprint 由 `prts-core` 决定，SQL 只执行 typed plan。

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use prts_common::Error;
use prts_core::permission::nodes;
use prts_core::search_query::{
    plan_structured_search, SearchQueryError, SearchScope, StructuredSearchPlan,
    StructuredSearchRequest,
};
use prts_search::orchestrator::{run, OrchestratorInput};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::auth::{project as paccess, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::routes::entries::EntryDto;
use crate::state::AppState;

const GET_SEARCH_SUNSET: &str = "Wed, 14 Jan 2027 00:00:00 GMT";
const SEARCH_RECALL_LIMIT: i64 = 2_000;
type HmacSha256 = Hmac<Sha256>;

/// deprecated GET 查询；只适配 all/file scope，不再支持 OFFSET 或第二套排序。
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub file_id: Option<i64>,
    /// 逗号分隔的 workflow state。
    pub state: Option<String>,
    #[serde(default)]
    pub include_hidden: bool,
    pub after: Option<String>,
    pub limit: Option<u16>,
}

/// OpenAPI condition operator shadow；运行时只反序列化 prts-core 类型。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum SearchOperatorSchema {
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Equals,
}

/// OpenAPI AND condition shadow。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct SearchConditionSchema {
    /// `source:<bcp47>`、`source_any`、`translation`、`key`。
    field: String,
    operator: SearchOperatorSchema,
    value: String,
}

/// OpenAPI scope shadow；精确描述五种 tagged-union JSON 形状。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum SearchScopeSchema {
    All {},
    Path { path: String },
    File { file_id: i64 },
    CurrentFile { file_id: i64 },
    CurrentTask { task_id: i64 },
}

/// OpenAPI 请求 shadow；运行时领域请求具有同一 JSON shape。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StructuredSearchRequestSchema {
    query: Option<String>,
    #[serde(default)]
    conditions: Vec<SearchConditionSchema>,
    scope: SearchScopeSchema,
    #[serde(default)]
    states: Vec<String>,
    #[serde(default)]
    include_hidden: bool,
    #[serde(default)]
    vector: bool,
    after: Option<String>,
    /// 默认 50，允许 1..=100。
    limit: Option<u16>,
}

/// 单条搜索结果；score 与 entry id 共同形成唯一稳定顺序。
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchHitDto {
    #[serde(flatten)]
    #[schema(inline)]
    pub entry: EntryDto,
    pub rrf_score: f64,
}

/// POST/GET 统一固定响应 envelope。
#[derive(Debug, Serialize, ToSchema)]
pub struct StructuredSearchResponse {
    pub items: Vec<SearchHitDto>,
    pub next_after: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchCursorV1 {
    version: u8,
    project_id: i64,
    fingerprint: String,
    last_rrf_score: f64,
    last_entry_id: i64,
}

/// 主结构化搜索接口。conditions 仅 AND；scope/condition unknown fields 返回 400。
#[utoipa::path(
    post,
    path = "/projects/{id}/search",
    tag = "search",
    summary = "结构化项目搜索",
    description = "在 URL 项目内执行 FTS、pg_trgm 与可选 pgvector 的 RRF 融合。scope 资源先做项目归属、active ancestor 与可见性验证；conditions/states/effective-visible 在 recall/fetch 保持一致。响应使用签名键集 cursor，不使用 OFFSET。",
    params(("id" = i64, Path, description = "项目 ID")),
    request_body = StructuredSearchRequestSchema,
    responses(
        (status = 200, body = StructuredSearchResponse, description = "固定 items/next_after envelope"),
        (status = 400, body = ErrorResponse, description = "请求、scope、condition、limit 或 cursor 无效"),
        (status = 403, body = ErrorResponse, description = "include_hidden 越权"),
        (status = 404, body = ErrorResponse, description = "项目或 scope 资源不存在/不可见"),
        (status = 409, body = ErrorResponse, description = "项目语言未解决或 lexical search 未 ready")
    )
)]
pub async fn structured_search(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(project_id): Path<i64>,
    request: Result<Json<StructuredSearchRequest>, JsonRejection>,
) -> Result<Json<StructuredSearchResponse>, ApiError> {
    let request = request
        .map_err(|_| Error::validation("SEARCH_REQUEST_INVALID"))?
        .0;
    execute_search(&state, user.as_ref(), project_id, request)
        .await
        .map(Json)
}

/// 旧 GET 只把 file_id 映射为 file scope，否则映射 all；与 POST 共用 typed service/SQL。
#[utoipa::path(
    get,
    path = "/projects/{id}/search",
    tag = "search",
    summary = "旧版项目搜索（已弃用）",
    description = "兼容一个发布周期。file_id 存在时映射为 file scope，否则映射 all；不会制造 current_file/current_task。响应带 Deprecation 与 Sunset headers。",
    params(("id" = i64, Path, description = "项目 ID"), SearchQuery),
    responses(
        (status = 200, body = StructuredSearchResponse),
        (status = 400, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 409, body = ErrorResponse)
    )
)]
#[deprecated(note = "use POST /projects/{id}/search")]
pub async fn search_entries(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(project_id): Path<i64>,
    Query(query): Query<SearchQuery>,
) -> Result<Response, ApiError> {
    let states = super::parse_states(query.state.as_deref())
        .into_iter()
        .filter_map(|state| prts_core::EntryState::parse(&state))
        .collect();
    let request = StructuredSearchRequest {
        query: query.q,
        conditions: Vec::new(),
        scope: query
            .file_id
            .map_or(SearchScope::All, |file_id| SearchScope::File { file_id }),
        states,
        include_hidden: query.include_hidden,
        vector: false,
        after: query.after,
        limit: query.limit.unwrap_or(50),
    };
    let body = execute_search(&state, user.as_ref(), project_id, request).await?;
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert("Deprecation", HeaderValue::from_static("true"));
    response
        .headers_mut()
        .insert("Sunset", HeaderValue::from_static(GET_SEARCH_SUNSET));
    response.headers_mut().insert(
        header::LINK,
        HeaderValue::from_static("</projects/{id}/search>; rel=successor-version"),
    );
    Ok(response)
}

async fn execute_search(
    state: &AppState,
    user: Option<&crate::auth::CurrentUser>,
    project_id: i64,
    request: StructuredSearchRequest,
) -> Result<StructuredSearchResponse, ApiError> {
    let access = paccess::load(state, user, project_id).await?;
    access.require_view()?;
    access.require_language_ready()?;
    if prts_db::terms::project_pending_deletion(&state.db, project_id)
        .await
        .map_err(db_err)?
    {
        return Err(Error::ProjectPendingDeletion.into());
    }
    match access.project.lexical_state.as_str() {
        "ready" => {}
        "failed" => {
            return Err(ApiError::from(Error::validation("PROJECT_SEARCH_FAILED"))
                .with_job_id(access.project.lexical_job_id))
        }
        _ => {
            return Err(
                ApiError::from(Error::validation("PROJECT_SEARCH_REBUILDING"))
                    .with_job_id(access.project.lexical_job_id),
            )
        }
    }
    if request.include_hidden && !access.has_node(nodes::PROJECT_ENTRY_HIDE) {
        return Err(Error::Forbidden.into());
    }
    let plan = plan_structured_search(&request, &access.project.source_langs)
        .map_err(search_query_error)?;
    let (file_ids, restrict_to_file_ids) = resolve_scope(state, project_id, &plan.scope).await?;
    let state_filters = plan
        .states
        .iter()
        .map(|state| state.as_str().to_string())
        .collect::<Vec<_>>();
    let fingerprint = fingerprint(project_id, &plan);
    let cursor = request
        .after
        .as_deref()
        .map(|cursor| {
            decode_cursor(
                cursor,
                &state.settings.auth.jwt_secret,
                project_id,
                &fingerprint,
            )
        })
        .transpose()?;
    let filter = prts_db::search::SearchExecutionFilter {
        file_ids: &file_ids,
        restrict_to_file_ids,
        states: &state_filters,
        conditions: &plan.conditions,
        include_hidden: plan.include_hidden,
    };

    let vector_ids = if plan.vector && access.project.embedding_state == "ready" {
        let runtime = state.search_rt.read().await.clone();
        match (
            runtime.embedding_enabled,
            state.embedder.as_ref().as_ref(),
            plan.query.as_ref(),
        ) {
            (true, Some(provider), Some(query)) => match provider
                .embed_batch(
                    &runtime.embedding_base_url,
                    &runtime.embedding_model,
                    std::slice::from_ref(query),
                )
                .await
            {
                Ok(mut vectors) if !vectors.is_empty() => {
                    let vector = vectors.remove(0);
                    prts_db::search::vector_search(
                        &state.db,
                        project_id,
                        &vector,
                        &filter,
                        SEARCH_RECALL_LIMIT,
                    )
                    .await
                    .ok()
                }
                _ => {
                    tracing::warn!("query embedding unavailable; degrading to lexical search");
                    None
                }
            },
            _ => None,
        }
    } else {
        None
    };

    let source_language = access
        .project
        .primary_source_lang
        .as_deref()
        .ok_or(Error::ProjectLanguageResolutionRequired)?;
    let mut results = run(
        &state.db,
        OrchestratorInput {
            project_id,
            query: plan.query.as_deref(),
            src_lang: source_language,
            tgt_lang: &access.project.target_lang,
            file_ids: &file_ids,
            restrict_to_file_ids,
            states: &state_filters,
            conditions: &plan.conditions,
            include_hidden: plan.include_hidden,
            per_path: SEARCH_RECALL_LIMIT,
            top_k: if plan.query.is_none() {
                i64::from(plan.limit) + 1
            } else {
                SEARCH_RECALL_LIMIT
            },
            filter_after_entry_id: if plan.query.is_none() {
                cursor.as_ref().map(|cursor| cursor.last_entry_id)
            } else {
                None
            },
            vector_ids,
        },
    )
    .await
    .map_err(db_err)?;
    if let Some(cursor) = cursor {
        results.retain(|(entry, score)| {
            *score < cursor.last_rrf_score
                || (*score == cursor.last_rrf_score && entry.id > cursor.last_entry_id)
        });
    }
    let limit = usize::from(plan.limit);
    let has_more = results.len() > limit;
    results.truncate(limit);
    let next_after = if has_more {
        results.last().map(|(entry, score)| {
            encode_cursor(
                &SearchCursorV1 {
                    version: 1,
                    project_id,
                    fingerprint: fingerprint.clone(),
                    last_rrf_score: *score,
                    last_entry_id: entry.id,
                },
                &state.settings.auth.jwt_secret,
            )
        })
    } else {
        None
    };
    Ok(StructuredSearchResponse {
        items: results
            .into_iter()
            .map(|(entry, score)| SearchHitDto {
                entry: EntryDto::from(&entry),
                rrf_score: score,
            })
            .collect(),
        next_after,
    })
}

async fn resolve_scope(
    state: &AppState,
    project_id: i64,
    scope: &SearchScope,
) -> Result<(Vec<i64>, bool), ApiError> {
    match scope {
        SearchScope::All => Ok((Vec::new(), false)),
        SearchScope::Path { path } => {
            match prts_db::search::resolve_path_scope(&state.db, project_id, path)
                .await
                .map_err(db_err)?
            {
                prts_db::search::PathScopeResolution::Missing => Err(Error::NotFound.into()),
                prts_db::search::PathScopeResolution::Ambiguous => {
                    Err(Error::validation("SEARCH_SCOPE_AMBIGUOUS").into())
                }
                prts_db::search::PathScopeResolution::Files(file_ids) => Ok((file_ids, true)),
            }
        }
        SearchScope::File { file_id } | SearchScope::CurrentFile { file_id } => {
            let file_id = prts_db::search::resolve_active_file_id(&state.db, project_id, *file_id)
                .await
                .map_err(db_err)?
                .ok_or(Error::NotFound)?;
            Ok((vec![file_id], true))
        }
        SearchScope::CurrentTask { task_id } => {
            let file_ids =
                prts_db::search::resolve_active_task_file_ids(&state.db, project_id, *task_id)
                    .await
                    .map_err(db_err)?
                    .ok_or(Error::NotFound)?;
            Ok((file_ids, true))
        }
    }
}

fn search_query_error(error: SearchQueryError) -> ApiError {
    Error::validation(error.code()).into()
}

fn fingerprint(project_id: i64, plan: &StructuredSearchPlan) -> String {
    let mut digest = Sha256::new();
    digest.update(b"prts-structured-search-fingerprint-v1\0");
    digest.update(project_id.to_be_bytes());
    digest.update(plan.fingerprint_material());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn encode_cursor(cursor: &SearchCursorV1, secret: &str) -> String {
    let payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(cursor).expect("search cursor payload must serialize"));
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts arbitrary cursor secret length");
    mac.update(b"prts-search-cursor-v1\0");
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{payload}.{signature}")
}

fn decode_cursor(
    value: &str,
    secret: &str,
    project_id: i64,
    fingerprint: &str,
) -> Result<SearchCursorV1, ApiError> {
    let (payload, signature) = value
        .split_once('.')
        .ok_or_else(|| Error::validation("SEARCH_CURSOR_INVALID"))?;
    if signature.contains('.') {
        return Err(Error::validation("SEARCH_CURSOR_INVALID").into());
    }
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| Error::validation("SEARCH_CURSOR_INVALID"))?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts arbitrary cursor secret length");
    mac.update(b"prts-search-cursor-v1\0");
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| Error::validation("SEARCH_CURSOR_INVALID"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Error::validation("SEARCH_CURSOR_INVALID"))?;
    let cursor: SearchCursorV1 =
        serde_json::from_slice(&payload).map_err(|_| Error::validation("SEARCH_CURSOR_INVALID"))?;
    if cursor.version != 1
        || cursor.project_id != project_id
        || cursor.fingerprint != fingerprint
        || !cursor.last_rrf_score.is_finite()
        || cursor.last_entry_id <= 0
    {
        return Err(Error::validation("SEARCH_CURSOR_INVALID").into());
    }
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prts_core::search_query::{SearchCondition, SearchOperator};

    #[test]
    fn cursor_round_trip_rejects_tamper_cross_project_query_and_version() {
        let plan = plan_structured_search(
            &StructuredSearchRequest {
                query: Some("hello".into()),
                conditions: vec![SearchCondition {
                    field: "translation".into(),
                    operator: SearchOperator::Contains,
                    value: "x".into(),
                }],
                scope: SearchScope::All,
                states: Vec::new(),
                include_hidden: false,
                vector: false,
                after: None,
                limit: 50,
            },
            &["en".into()],
        )
        .unwrap();
        let fingerprint = fingerprint(1, &plan);
        let cursor = SearchCursorV1 {
            version: 1,
            project_id: 1,
            fingerprint: fingerprint.clone(),
            last_rrf_score: 0.5,
            last_entry_id: 9,
        };
        let encoded = encode_cursor(&cursor, "secret");
        let decoded = decode_cursor(&encoded, "secret", 1, &fingerprint);
        assert!(decoded.is_ok());
        assert_eq!(decoded.ok().map(|cursor| cursor.last_entry_id), Some(9));
        let mut tampered = encoded.into_bytes();
        tampered[0] = if tampered[0] == b'a' { b'b' } else { b'a' };
        assert!(decode_cursor(
            std::str::from_utf8(&tampered).unwrap(),
            "secret",
            1,
            &fingerprint
        )
        .is_err());
        let encoded = encode_cursor(&cursor, "secret");
        assert!(decode_cursor(&encoded, "secret", 2, &fingerprint).is_err());
        assert!(decode_cursor(&encoded, "secret", 1, "different").is_err());
        let invalid_version = SearchCursorV1 {
            version: 2,
            ..cursor
        };
        assert!(decode_cursor(
            &encode_cursor(&invalid_version, "secret"),
            "secret",
            1,
            &fingerprint
        )
        .is_err());
    }

    #[test]
    fn vector_false_gate_precedes_every_embedding_call_and_logs_no_query_body() {
        let source = include_str!("search.rs");
        let vector_gate = source.find("if plan.vector").unwrap();
        let embed_call = source.find(".embed_batch(").unwrap();
        assert!(vector_gate < embed_call);
        assert!(!source.contains("tracing::warn!(\"{query}"));
        assert!(!source.contains("tracing::info!(\"{query}"));
        assert!(!source.contains("tracing::error!(\"{query}"));
    }

    #[test]
    fn project_pending_deletion_is_checked_before_search_execution() {
        let source = include_str!("search.rs");
        let pending_gate = source.find("project_pending_deletion").unwrap();
        let orchestrator = source.find("let mut results = run(").unwrap();
        assert!(pending_gate < orchestrator);
    }
}

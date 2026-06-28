//! 项目与成员端点。

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::permission::nodes;
use prts_core::ProjectRole;

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::ApiError;
use crate::state::AppState;

/// 项目对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectDto {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub visibility: String,
    pub source_langs: Vec<String>,
    pub target_lang: String,
    pub owner_id: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&prts_db::models::Project> for ProjectDto {
    fn from(p: &prts_db::models::Project) -> Self {
        Self {
            id: p.id,
            slug: p.slug.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            visibility: p.visibility.clone(),
            source_langs: p.source_langs.clone(),
            target_lang: p.target_lang.clone(),
            owner_id: p.owner_id,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

/// 项目详情（含各状态词条统计）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectDetailDto {
    pub project: ProjectDto,
    pub state_counts: HashMap<String, i64>,
    pub entry_count: i64,
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

async fn unique_slug(state: &AppState, base: &str) -> Result<String, ApiError> {
    let base = if base.is_empty() { "project" } else { base };
    if !prts_db::projects::slug_exists(&state.db, base)
        .await
        .map_err(db_err)?
    {
        return Ok(base.to_string());
    }
    for i in 2..10_000 {
        let candidate = format!("{base}-{i}");
        if !prts_db::projects::slug_exists(&state.db, &candidate)
            .await
            .map_err(db_err)?
        {
            return Ok(candidate);
        }
    }
    Ok(format!(
        "{base}-{}",
        prts_auth::token::random_token(6).to_lowercase()
    ))
}

/// 创建项目请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProjectReq {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// public | private（缺省 public）。
    #[serde(default)]
    pub visibility: Option<String>,
    pub source_langs: Vec<String>,
    pub target_lang: String,
}

/// 创建项目（创建者成为拥有者）。需平台「创建项目」权限。
#[utoipa::path(post, path = "/projects", tag = "project", request_body = CreateProjectReq,
    responses((status = 200, body = ProjectDto), (status = 400), (status = 403)))]
pub async fn create_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateProjectReq>,
) -> Result<Json<ProjectDto>, ApiError> {
    user.require_platform(nodes::PLATFORM_PROJECT_CREATE)?;

    let name = req.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(Error::bad_request("项目名需为 1–128 字符").into());
    }
    let target_lang = req.target_lang.trim();
    if target_lang.is_empty() {
        return Err(Error::bad_request("需指定目标语言").into());
    }
    let base = req
        .slug
        .as_deref()
        .map(slugify)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify(name));
    let slug = unique_slug(&state, &base).await?;
    let visibility = match req.visibility.as_deref() {
        Some("private") => "private",
        _ => "public",
    };

    let project = prts_db::projects::create(
        &state.db,
        &slug,
        name,
        req.description.as_deref().unwrap_or(""),
        visibility,
        &req.source_langs,
        target_lang,
        user.id,
    )
    .await
    .map_err(db_err)?;
    prts_db::memberships::upsert(&state.db, project.id, user.id, "owner")
        .await
        .map_err(db_err)?;

    Ok(Json((&project).into()))
}

/// 列表查询参数。
#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    #[serde(default)]
    pub mine: bool,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub per_page: Option<i64>,
}

/// 列出项目：`mine=true` 列出我参与的（需登录），否则列出公开项目。
#[utoipa::path(get, path = "/projects", tag = "project",
    responses((status = 200, body = [ProjectDto])))]
pub async fn list_projects(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Query(q): Query<ListProjectsQuery>,
) -> Result<Json<Vec<ProjectDto>>, ApiError> {
    let projects = if q.mine {
        let u = user.as_ref().ok_or(Error::Unauthorized)?;
        prts_db::projects::list_for_user(&state.db, u.id)
            .await
            .map_err(db_err)?
    } else {
        let per = q.per_page.unwrap_or(50).clamp(1, 200);
        let page = q.page.unwrap_or(1).max(1);
        prts_db::projects::list_public(&state.db, per, (page - 1) * per)
            .await
            .map_err(db_err)?
    };
    Ok(Json(projects.iter().map(ProjectDto::from).collect()))
}

/// 项目详情。
#[utoipa::path(get, path = "/projects/{id}", tag = "project",
    responses((status = 200, body = ProjectDetailDto), (status = 404)))]
pub async fn get_project(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
) -> Result<Json<ProjectDetailDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    let counts = prts_db::entries::count_by_state(&state.db, id)
        .await
        .map_err(db_err)?;
    let mut state_counts = HashMap::new();
    let mut entry_count = 0i64;
    for (s, c) in counts {
        entry_count += c;
        state_counts.insert(s, c);
    }

    Ok(Json(ProjectDetailDto {
        project: (&access.project).into(),
        state_counts,
        entry_count,
    }))
}

/// 更新项目请求（字段缺省表示不变）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProjectReq {
    pub name: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
    pub source_langs: Option<Vec<String>>,
    pub target_lang: Option<String>,
}

/// 更新项目元信息。需项目「管理」权限。
#[utoipa::path(put, path = "/projects/{id}", tag = "project", request_body = UpdateProjectReq,
    responses((status = 200, body = ProjectDto), (status = 403), (status = 404)))]
pub async fn update_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProjectReq>,
) -> Result<Json<ProjectDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let p = &access.project;

    let visibility = match req.visibility.as_deref() {
        Some("private") => "private".to_string(),
        Some("public") => "public".to_string(),
        _ => p.visibility.clone(),
    };
    let updated = prts_db::projects::update(
        &state.db,
        id,
        req.name.as_deref().unwrap_or(&p.name),
        req.description.as_deref().unwrap_or(&p.description),
        &visibility,
        req.source_langs.as_deref().unwrap_or(&p.source_langs),
        req.target_lang.as_deref().unwrap_or(&p.target_lang),
    )
    .await
    .map_err(db_err)?;
    Ok(Json((&updated).into()))
}

/// 删除项目。需项目「删除」权限。
#[utoipa::path(delete, path = "/projects/{id}", tag = "project",
    responses((status = 204), (status = 403), (status = 404)))]
pub async fn delete_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_DELETE)?;
    prts_db::projects::delete(&state.db, id)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 成员对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberDto {
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: String,
}

/// 列出项目成员。
#[utoipa::path(get, path = "/projects/{id}/members", tag = "project",
    responses((status = 200, body = [MemberDto]), (status = 404)))]
pub async fn list_members(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
) -> Result<Json<Vec<MemberDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    let members = prts_db::memberships::list(&state.db, id)
        .await
        .map_err(db_err)?;
    Ok(Json(
        members
            .into_iter()
            .map(|m| MemberDto {
                user_id: m.user_id,
                username: m.username,
                avatar_url: m.avatar_url,
                role: m.role,
                created_at: m.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

/// 添加/更新成员请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddMemberReq {
    pub username: String,
    /// owner | manager | reviewer | translator。
    pub role: String,
}

/// 添加或更新项目成员。需项目「成员管理」权限。
#[utoipa::path(post, path = "/projects/{id}/members", tag = "project", request_body = AddMemberReq,
    responses((status = 204), (status = 400), (status = 403), (status = 404)))]
pub async fn add_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<AddMemberReq>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    if ProjectRole::parse(&req.role).is_none() {
        return Err(Error::bad_request("role 必须是 owner|manager|reviewer|translator").into());
    }
    let target = prts_db::users::find_by_username(&state.db, req.username.trim())
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    prts_db::memberships::upsert(&state.db, id, target.id, &req.role)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 移除项目成员。需项目「成员管理」权限。不可移除最后一个拥有者。
#[utoipa::path(delete, path = "/projects/{id}/members/{user_id}", tag = "project",
    responses((status = 204), (status = 400), (status = 403), (status = 404)))]
pub async fn remove_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, user_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;

    // 不可移除最后一个 owner
    if let Some(role) = prts_db::memberships::find_role(&state.db, id, user_id)
        .await
        .map_err(db_err)?
    {
        if role == "owner"
            && prts_db::memberships::count_role(&state.db, id, "owner")
                .await
                .map_err(db_err)?
                <= 1
        {
            return Err(Error::bad_request("不能移除最后一个拥有者").into());
        }
    }
    if prts_db::memberships::remove(&state.db, id, user_id)
        .await
        .map_err(db_err)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::NotFound.into())
    }
}

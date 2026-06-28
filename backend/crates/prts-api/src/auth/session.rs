//! 会话令牌：签发 access(JWT) + refresh(不透明，存 Redis，可吊销、刷新时轮换)。

use prts_auth::{jwt, token};
use prts_common::Error;

use crate::state::AppState;

/// 一次签发的令牌对。
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// access token 剩余有效秒数。
    pub access_expires_in: i64,
    /// 令牌所属用户 id。
    pub user_id: i64,
}

fn refresh_key(refresh_token: &str) -> String {
    format!("refresh:{}", token::sha256_hex(refresh_token))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn redis_err(e: redis::RedisError) -> Error {
    Error::internal(format!("redis error: {e}"))
}

/// 为用户签发新的令牌对，并把 refresh 哈希写入 Redis（带 TTL）。
pub async fn issue(state: &AppState, user_id: i64) -> Result<IssuedTokens, Error> {
    let auth = &state.settings.auth;
    let iat = now();
    let claims = jwt::Claims {
        sub: user_id,
        iat,
        exp: iat + auth.access_ttl_secs,
        typ: "access".to_string(),
    };
    let access_token = jwt::encode(&claims, state.jwt_secret());
    let refresh_token = token::random_token(48);

    let mut conn = state.cache.clone();
    let _: () = redis::cmd("SET")
        .arg(refresh_key(&refresh_token))
        .arg(user_id)
        .arg("EX")
        .arg(auth.refresh_ttl_secs)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;

    Ok(IssuedTokens {
        access_token,
        refresh_token,
        access_expires_in: auth.access_ttl_secs,
        user_id,
    })
}

/// 用 refresh token 轮换出新令牌对（旧 refresh 立即失效）。
pub async fn refresh(state: &AppState, refresh_token: &str) -> Result<IssuedTokens, Error> {
    let key = refresh_key(refresh_token);
    let mut conn = state.cache.clone();

    let user_id: Option<i64> = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;
    let user_id = user_id.ok_or(Error::Unauthorized)?;

    // 轮换：删除旧 refresh，签发新的。
    let _: () = redis::cmd("DEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;

    issue(state, user_id).await
}

/// 吊销 refresh token（登出）。幂等。
pub async fn revoke(state: &AppState, refresh_token: &str) -> Result<(), Error> {
    let mut conn = state.cache.clone();
    let _: () = redis::cmd("DEL")
        .arg(refresh_key(refresh_token))
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;
    Ok(())
}

/// 暂存 OAuth 流程的 `state → code_verifier`（带 TTL，默认 10 分钟）。
pub async fn store_oauth_state(
    state: &AppState,
    oauth_state: &str,
    code_verifier: &str,
) -> Result<(), Error> {
    let mut conn = state.cache.clone();
    let _: () = redis::cmd("SET")
        .arg(format!("oauth_state:{oauth_state}"))
        .arg(code_verifier)
        .arg("EX")
        .arg(600)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;
    Ok(())
}

/// 取回并删除 OAuth 流程暂存的 `code_verifier`（一次性）。
pub async fn take_oauth_state(
    state: &AppState,
    oauth_state: &str,
) -> Result<Option<String>, Error> {
    let key = format!("oauth_state:{oauth_state}");
    let mut conn = state.cache.clone();
    let verifier: Option<String> = redis::cmd("GETDEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;
    Ok(verifier)
}

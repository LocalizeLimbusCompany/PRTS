//! DB 集成测试：对真实 PostgreSQL 跑通仓储层 SQL 与迁移。
//!
//! 仅在 `db-tests` 特性下编译，并需要环境变量 `DATABASE_URL`。
//! 本地无 DB 时默认不编译；CI 会起 Postgres 服务后执行（见 .github/workflows/ci.yml）。
#![cfg(feature = "db-tests")]

use prts_db::{api_keys, settings, users};

async fn pool() -> prts_db::Db {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL 未设置");
    let pool = prts_db::connect_postgres(&url, 5)
        .await
        .expect("连接 Postgres");
    prts_db::run_migrations(&pool).await.expect("执行迁移");
    pool
}

#[tokio::test]
async fn users_api_keys_settings_oauth_roundtrip() {
    let pool = pool().await;

    // 清理上次残留
    sqlx::query("DELETE FROM users WHERE username IN ('itest_user', 'itest_oauth')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM settings WHERE key = 'test.flag'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 账号密码用户 ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let user = users::create_password_user(
        &pool,
        "itest_user",
        Some("itest@example.com"),
        &hash,
        "active",
    )
    .await
    .unwrap();
    assert_eq!(user.username, "itest_user");
    assert!(user.password_hash.is_some());
    assert!(users::username_exists(&pool, "itest_user").await.unwrap());

    let found = users::find_by_username(&pool, "itest_user")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, user.id);

    let updated = users::update_profile(
        &pool,
        user.id,
        "hi",
        Some("http://x/a.png"),
        &["en".to_string(), "zh-Hans".to_string()],
    )
    .await
    .unwrap();
    assert_eq!(updated.description, "hi");
    assert_eq!(
        updated.translation_langs,
        vec!["en".to_string(), "zh-Hans".to_string()]
    );

    users::set_platform_role(&pool, user.id, Some("admin"))
        .await
        .unwrap();
    let admin = users::find_by_id(&pool, user.id).await.unwrap().unwrap();
    assert_eq!(admin.platform_role.as_deref(), Some("admin"));

    // —— API Key ——
    let key = prts_auth::token::generate_api_key();
    let rec = api_keys::create(&pool, user.id, "k", &key.hash, &key.display_prefix)
        .await
        .unwrap();
    let by_hash = api_keys::find_user_by_key_hash(&pool, &key.hash)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_hash.id, user.id);
    assert!(api_keys::revoke(&pool, user.id, rec.id).await.unwrap());
    assert!(!api_keys::revoke(&pool, user.id, rec.id).await.unwrap()); // 二次删除无效

    // —— 设置 ——
    settings::set(&pool, "test.flag", &serde_json::json!(true), Some(user.id))
        .await
        .unwrap();
    assert_eq!(
        settings::get(&pool, "test.flag").await.unwrap().unwrap(),
        serde_json::json!(true)
    );

    // —— OAuth 用户 + 关联账号 ——
    let extra = serde_json::json!({ "work_scope": "英翻" });
    let ouser = users::create_oauth_user(
        &pool,
        "itest_oauth",
        Some("http://x/b.png"),
        "zoot",
        "ext-123",
        &extra,
    )
    .await
    .unwrap();
    assert!(ouser.password_hash.is_none());
    let by_ext = users::find_by_external(&pool, "zoot", "ext-123")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_ext.id, ouser.id);
    let accounts = users::list_external_accounts(&pool, ouser.id)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].provider, "zoot");
    assert_eq!(accounts[0].raw.get("work_scope").unwrap(), "英翻");

    // 清理
    sqlx::query("DELETE FROM users WHERE username IN ('itest_user', 'itest_oauth')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM settings WHERE key = 'test.flag'")
        .execute(&pool)
        .await
        .unwrap();
}

//! DB 集成测试：对真实 PostgreSQL 跑通仓储层 SQL 与迁移。
//!
//! 仅在 `db-tests` 特性下编译，并需要环境变量 `DATABASE_URL`。
//! 本地无 DB 时默认不编译；CI 会起 Postgres 服务后执行（见 .github/workflows/ci.yml）。
#![cfg(feature = "db-tests")]

use prts_db::{api_keys, entries, files, memberships, notifications, projects, settings, users};

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

#[tokio::test]
async fn projects_files_entries_roundtrip() {
    let pool = pool().await;

    sqlx::query("DELETE FROM projects WHERE slug = 'itest-proj'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_owner'")
        .execute(&pool)
        .await
        .unwrap();

    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_owner", None, &hash, "active")
        .await
        .unwrap();

    // 项目 + 成员
    let proj = projects::create(
        &pool,
        "itest-proj",
        "ITest",
        "",
        "public",
        &["en".to_string(), "ja".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&pool, proj.id, owner.id, "owner")
        .await
        .unwrap();
    assert_eq!(
        memberships::find_role(&pool, proj.id, owner.id)
            .await
            .unwrap()
            .as_deref(),
        Some("owner")
    );
    assert!(projects::slug_exists(&pool, "itest-proj").await.unwrap());

    // 按路径上传（自动建文件夹/文件）
    let file = files::ensure_file_at_path(&pool, proj.id, "dialog/ch1.json")
        .await
        .unwrap();
    assert_eq!(file.path, "dialog/ch1.json");

    let initial = vec![
        entries::UploadEntry {
            key: "k1".to_string(),
            original: serde_json::json!({ "en": "Hello", "ja": "こんにちは" }),
            context: Some("greeting".to_string()),
            translation: None,
            state: None,
        },
        entries::UploadEntry {
            key: "k2".to_string(),
            original: serde_json::json!({ "en": "Bye" }),
            context: None,
            translation: None,
            state: None,
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &initial, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2);
    files::refresh_entry_count(&pool, file.id).await.unwrap();

    let listed = entries::list(&pool, proj.id, &entries::EntryFilter::default(), None, 100)
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
    let k1 = listed.iter().find(|e| e.key == "k1").unwrap().clone();
    assert_eq!(k1.state, "untranslated");

    // 乐观锁更新
    let updated = entries::update_translation(
        &pool,
        k1.id,
        k1.version,
        "你好",
        "translated",
        "translate",
        Some(owner.id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(updated.translation, "你好");
    assert_eq!(updated.state, "translated");
    // 用过期版本号 → 冲突
    let conflict = entries::update_translation(
        &pool,
        k1.id,
        k1.version,
        "x",
        "translated",
        "edit",
        Some(owner.id),
    )
    .await
    .unwrap();
    assert!(conflict.is_none());

    // 重传：同 key 源文变化 → 保留译文、置未翻译、记历史
    let reupload = vec![entries::UploadEntry {
        key: "k1".to_string(),
        original: serde_json::json!({ "en": "Hello!", "ja": "こんにちは" }),
        context: None,
        translation: None,
        state: None,
    }];
    let stats2 = entries::bulk_upsert(&pool, file.id, proj.id, &reupload, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats2.updated, 1);
    let k1b = entries::get(&pool, proj.id, k1.id).await.unwrap().unwrap();
    assert_eq!(k1b.state, "untranslated"); // 已重置
    assert_eq!(k1b.translation, "你好"); // 译文保留
    let history = entries::list_versions(&pool, k1.id, 50).await.unwrap();
    assert!(history.iter().any(|v| v.kind == "source_update"));

    // 锁定标志
    let locked = entries::set_flags(&pool, proj.id, k1.id, Some(true), None)
        .await
        .unwrap()
        .unwrap();
    assert!(locked.locked);

    // 导出列表
    let export = entries::list_for_export(&pool, proj.id).await.unwrap();
    assert_eq!(export.len(), 2);

    // 级联清理
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证迁移 0004 的触发器：
/// - 插入中文源词条后，`source_text` 由触发器从 `original` JSON 取出；
/// - `source_tsv` 经 zhparser 分词，长度大于 0（zhparser 在 CI Postgres 镜像中可用）。
///
/// 仅在 CI 环境（带 zhparser 的 Postgres 镜像）运行；本地无 DB 时只编译不执行。
#[tokio::test]
async fn migration_0004_trigger_populates_source_text_and_zhparser_tsv() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-zh-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_zh_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner（owner_id NOT NULL REFERENCES users） ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_zh_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目：source_langs 首位为 'zh-Hans'，触发器用 source_langs[1] 提取 ——
    let proj = projects::create(
        &pool,
        "itest-zh-search",
        "ITest ZH Search",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "search/test.json")
        .await
        .unwrap();

    // —— 插入中文原文词条，state='translated' ——
    // original 的键必须与 source_langs[1]（即 'zh-Hans'）一致，触发器才能提取源文本。
    let batch = vec![entries::UploadEntry {
        key: "zh_test_key".to_string(),
        original: serde_json::json!({ "zh-Hans": "今天天气很好" }),
        context: None,
        translation: Some("nice weather".to_string()),
        state: Some("translated".to_string()),
    }];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 1, "应创建 1 条词条");

    // —— 验证触发器效果 ——
    // source_text：触发器从 original->>'zh-Hans' 填充
    // source_tsv：触发器对中文源文用 zhparser 分词（prts_zh 配置），CI 必有 zhparser
    let (source_text, tsv_text): (String, String) = sqlx::query_as(
        "SELECT source_text, source_tsv::text FROM entries WHERE project_id = $1 AND key = 'zh_test_key'",
    )
    .bind(proj.id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        source_text, "今天天气很好",
        "触发器应将 original->>'zh-Hans' 写入 source_text"
    );
    assert!(
        !tsv_text.is_empty(),
        "zhparser 应将中文分词写入 source_tsv（实际得到空串，请确认 zhparser 已安装）"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_zh_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 prts-db::search 的 trgm_search 和 fts_search：
/// - trgm_search 按三元组相似度召回匹配 "weather" 的词条；
/// - fts_search 按 plainto_tsquery 匹配英文译文中的 "weather"。
#[tokio::test]
async fn search_trgm_and_fts_recall() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-search-recall'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_search_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_search_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目：source_langs 首位 'zh-Hans'，目标语言 'en' ——
    let proj = projects::create(
        &pool,
        "itest-search-recall",
        "ITest Search Recall",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "search/recall.json")
        .await
        .unwrap();

    // —— 插入两条词条 ——
    let batch = vec![
        entries::UploadEntry {
            key: "w1".to_string(),
            original: serde_json::json!({ "zh-Hans": "今天天气不错" }),
            context: None,
            translation: Some("nice weather today".to_string()),
            state: Some("translated".to_string()),
        },
        entries::UploadEntry {
            key: "w2".to_string(),
            original: serde_json::json!({ "zh-Hans": "完全无关的内容" }),
            context: None,
            translation: Some("completely unrelated".to_string()),
            state: Some("translated".to_string()),
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2, "应创建 2 条词条");

    // —— 查询 w1/w2 的真实 id ——
    let w1_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'w1'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let w2_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'w2'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— trgm 召回：w1 的 translation 含 "weather"，应出现在结果中 ——
    let trgm_ids = prts_db::search::trgm_search(&pool, proj.id, "weather", &[], &[], false, 10)
        .await
        .unwrap();
    assert!(
        trgm_ids.contains(&w1_id),
        "trgm_search 应召回 w1（translation 含 'weather'），实际结果：{trgm_ids:?}"
    );
    assert!(
        !trgm_ids.contains(&w2_id),
        "无关词条不应出现在 'weather' 的 trgm 结果中，实际结果：{trgm_ids:?}"
    );

    // —— FTS 召回：英文 plainto_tsquery('english', 'weather') 应匹配 w1 的 translation_tsv ——
    let fts_ids = prts_db::search::fts_search(
        &pool,
        proj.id,
        "weather",
        "zh-Hans",
        "en",
        &[],
        &[],
        false,
        10,
    )
    .await
    .unwrap();
    assert!(
        fts_ids.contains(&w1_id),
        "fts_search 应召回 w1（translation_tsv 匹配 'weather'），实际结果：{fts_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_search_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 prts-search::orchestrator::run 端到端：
/// - 给定一个含已翻译词条的项目，orchestrator 应为匹配查询词的词条返回命中；
/// - 对无关词条不应出现在结果中；
/// - 结果中应包含 (Entry, relevance) 元组，relevance > 0。
#[tokio::test]
async fn search_orchestrator_returns_ranked_hits() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-orch-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_orch_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_orch_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目：源语言 'zh-Hans'，目标 'en' ——
    let proj = projects::create(
        &pool,
        "itest-orch-search",
        "ITest Orchestrator Search",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件并插入词条 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "orch/test.json")
        .await
        .unwrap();

    let batch = vec![
        entries::UploadEntry {
            key: "orch1".to_string(),
            original: serde_json::json!({ "zh-Hans": "明日之后" }),
            context: None,
            translation: Some("state of survival".to_string()),
            state: Some("translated".to_string()),
        },
        entries::UploadEntry {
            key: "orch2".to_string(),
            original: serde_json::json!({ "zh-Hans": "完全不相关词条" }),
            context: None,
            translation: Some("completely irrelevant entry".to_string()),
            state: Some("translated".to_string()),
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2, "应创建 2 条词条");

    let orch1_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'orch1'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let orch2_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'orch2'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— 调用 orchestrator.run ——
    let results = prts_search::orchestrator::run(
        &pool,
        prts_search::orchestrator::OrchestratorInput {
            project_id: proj.id,
            q: "survival",
            src_lang: "zh-Hans",
            tgt_lang: "en",
            file_ids: &[],
            states: &[],
            include_hidden: false,
            per_path: 100,
            top_k: 200,
            sort: prts_search::SortBy::Relevance,
            vector_ids: None,
        },
    )
    .await
    .unwrap();

    // 结果列表非空
    assert!(!results.is_empty(), "orchestrator 应返回至少一条命中");

    let hit_ids: Vec<i64> = results.iter().map(|(e, _)| e.id).collect();

    // "survival" 应命中 orch1（translation = "state of survival"）
    assert!(
        hit_ids.contains(&orch1_id),
        "orchestrator 应返回 orch1（translation 含 'survival'），实际结果 ids: {hit_ids:?}"
    );

    // 无关词条 orch2 不应出现在结果中
    assert!(
        !hit_ids.contains(&orch2_id),
        "无关词条 orch2 不应出现在 'survival' 的搜索结果中，实际结果 ids: {hit_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_orch_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 vector_search 的 cosine 距离排序：
/// - 播种两条词条，各手动 UPDATE embedding 为 1024 维已知向量；
/// - 查询向量贴近词条 A，断言 A 的 id 排在 B 之前。
///
/// 注：直接 UPDATE embedding 不改 original/source_text，故不触发清零触发器。
#[tokio::test]
async fn vector_search_returns_nearest_first() {
    use pgvector::Vector;

    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-vec-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_vec_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_vec_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目 ——
    let proj = projects::create(
        &pool,
        "itest-vec-search",
        "ITest Vec Search",
        "",
        "public",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件 + 插入两条词条 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "vec/test.json")
        .await
        .unwrap();

    let batch = vec![
        entries::UploadEntry {
            key: "va".to_string(),
            original: serde_json::json!({ "en": "vector entry A" }),
            context: None,
            translation: None,
            state: None,
        },
        entries::UploadEntry {
            key: "vb".to_string(),
            original: serde_json::json!({ "en": "vector entry B" }),
            context: None,
            translation: None,
            state: None,
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2, "应创建 2 条词条");

    // —— 取真实 id ——
    let id_a: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'va'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let id_b: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'vb'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— 构造 1024 维向量 ——
    // A 贴近 [1, 0, 0, ...0]，B 贴近 [0, 1, 0, ...0]
    let mut vec_a = vec![0.0_f32; 1024];
    vec_a[0] = 1.0;
    let mut vec_b = vec![0.0_f32; 1024];
    vec_b[1] = 1.0;

    // —— 直接 UPDATE embedding（不改 original，不触发清零触发器）——
    sqlx::query("UPDATE entries SET embedding = $1 WHERE id = $2")
        .bind(Vector::from(vec_a.clone()))
        .bind(id_a)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE entries SET embedding = $1 WHERE id = $2")
        .bind(Vector::from(vec_b))
        .bind(id_b)
        .execute(&pool)
        .await
        .unwrap();

    // —— 查询向量贴近 A，断言 A 排在 B 之前 ——
    let result_ids = prts_db::search::vector_search(&pool, proj.id, &vec_a, &[], &[], false, 10)
        .await
        .unwrap();

    assert!(!result_ids.is_empty(), "vector_search 应返回至少一条结果");
    let pos_a = result_ids.iter().position(|&x| x == id_a);
    let pos_b = result_ids.iter().position(|&x| x == id_b);
    assert!(
        pos_a.is_some() && pos_b.is_some(),
        "两条词条均应出现在结果中，实际：{result_ids:?}"
    );
    assert!(
        pos_a.unwrap() < pos_b.unwrap(),
        "A 应排在 B 之前（与查询向量 cosine 更近），实际顺序：{result_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_vec_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 search_settings::set 规范化 + get 持久化圆环：
/// - 写入超出安全区间的 embedding_batch = 99、tm_top_n = 9；
/// - 规范化后持久化，再 get 取回，应得 clamped 值（10 和 3）。
#[tokio::test]
async fn search_settings_set_and_get_normalizes_values() {
    use prts_db::search_settings::{self, SearchConfig};

    let pool = pool().await;

    // 清理上次残留（key 固定为 "search.config"）
    sqlx::query("DELETE FROM settings WHERE key = 'search.config'")
        .execute(&pool)
        .await
        .unwrap();

    // 写入超范围值
    let cfg = SearchConfig {
        embedding_batch: 99,
        tm_top_n: 9,
        ..SearchConfig::default()
    };
    search_settings::set(&pool, cfg, None).await.unwrap();

    // 读取并断言规范化结果
    let got = search_settings::get(&pool).await.unwrap();
    assert_eq!(
        got.embedding_batch, 10,
        "embedding_batch 应被 clamp 到上限 10"
    );
    assert_eq!(got.tm_top_n, 3, "tm_top_n 应被 clamp 到上限 3");

    // 清理
    sqlx::query("DELETE FROM settings WHERE key = 'search.config'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 prts-db::notifications 仓储圆环（迁移 0005）：
/// - 为收件人 `create` 两条通知 → `unread_count` == 2；
/// - `mark_read` 其中一条 → `unread_count` == 1；
/// - `list` 按 id 降序返回（较新的在前）。
#[tokio::test]
async fn notifications_repository_roundtrip() {
    let pool = pool().await;

    // —— 清理上次残留（级联删除该用户的通知） ——
    sqlx::query("DELETE FROM users WHERE username = 'itest_notif_user'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建收件人 ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let user = users::create_password_user(&pool, "itest_notif_user", None, &hash, "active")
        .await
        .unwrap();

    // —— 创建两条通知 ——
    let n1 = notifications::create(
        &pool,
        user.id,
        "poke",
        &serde_json::json!({ "text": "first" }),
    )
    .await
    .unwrap();
    let n2 = notifications::create(
        &pool,
        user.id,
        "poke",
        &serde_json::json!({ "text": "second" }),
    )
    .await
    .unwrap();
    assert_eq!(n1.kind, "poke");
    assert!(n1.read_at.is_none());
    assert!(n2.id > n1.id, "IDENTITY 应递增，n2.id 应大于 n1.id");

    // —— 两条均未读 ——
    assert_eq!(
        notifications::unread_count(&pool, user.id).await.unwrap(),
        2,
        "创建后应有 2 条未读"
    );

    // —— 标记 n1 已读 → 剩 1 条未读 ——
    notifications::mark_read(&pool, user.id, &[n1.id])
        .await
        .unwrap();
    assert_eq!(
        notifications::unread_count(&pool, user.id).await.unwrap(),
        1,
        "标记一条已读后应剩 1 条未读"
    );

    // —— list 按 id 降序（较新的 n2 在前） ——
    let listed = notifications::list(&pool, user.id, None, 100)
        .await
        .unwrap();
    assert_eq!(listed.len(), 2, "应列出全部 2 条");
    assert_eq!(listed[0].id, n2.id, "list 应按 id 降序：较新的 n2 在最前");
    assert_eq!(listed[1].id, n1.id);
    // n1 已读、n2 仍未读
    let n1_listed = listed.iter().find(|n| n.id == n1.id).unwrap();
    let n2_listed = listed.iter().find(|n| n.id == n2.id).unwrap();
    assert!(n1_listed.read_at.is_some(), "n1 应已读");
    assert!(n2_listed.read_at.is_none(), "n2 应仍未读");

    // —— 键集游标：before_id = n2.id 只返回更旧的 n1 ——
    let older = notifications::list(&pool, user.id, Some(n2.id), 100)
        .await
        .unwrap();
    assert_eq!(older.len(), 1, "游标 before=n2 应只返回更旧的 n1");
    assert_eq!(older[0].id, n1.id);

    // —— 级联清理 ——
    sqlx::query("DELETE FROM users WHERE username = 'itest_notif_user'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 suggestions_trgm 的成员范围过滤：
/// - 用户 U 是项目 A 和 B 的成员，不是项目 C 的成员；
/// - 查询 A 中某词条的 trgm 建议：应包含 B 的跨项目词条，排除 C 及 A 自身被查词条。
#[tokio::test]
async fn suggestions_trgm_membership_scoped() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    for slug in ["itest-tm-a", "itest-tm-b", "itest-tm-c"] {
        sqlx::query("DELETE FROM projects WHERE slug = $1")
            .bind(slug)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM users WHERE username = 'itest_tm_user'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建用户 U ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let u = users::create_password_user(&pool, "itest_tm_user", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目 A（U 是 owner，自动加入成员） ——
    let proj_a = projects::create(
        &pool,
        "itest-tm-a",
        "TM Test A",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        u.id,
    )
    .await
    .unwrap();
    // projects::create 已插入 owner 行，此处确保角色正确
    memberships::upsert(&pool, proj_a.id, u.id, "owner")
        .await
        .unwrap();

    // —— 建项目 B（U 作为 translator 加入） ——
    let proj_b = projects::create(
        &pool,
        "itest-tm-b",
        "TM Test B",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        u.id, // owner 也是 U，所以 U 在 B 中
    )
    .await
    .unwrap();
    memberships::upsert(&pool, proj_b.id, u.id, "translator")
        .await
        .unwrap();

    // —— 建项目 C（U 不是成员；由另一个 owner 创建，使用直接 INSERT） ——
    // 为了避免引入额外用户，直接插入项目并让 U 不加入成员表
    let proj_c: (i64,) = sqlx::query_as(
        "INSERT INTO projects (slug, name, description, visibility, source_langs, target_lang, owner_id)
         VALUES ('itest-tm-c', 'TM Test C', '', 'public', ARRAY['zh-Hans'], 'en', $1)
         RETURNING id",
    )
    .bind(u.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let proj_c_id = proj_c.0;
    // 明确不为 U 在 proj_c 插入 membership

    // —— 建文件 ——
    let file_a = files::ensure_file_at_path(&pool, proj_a.id, "tm/a.json")
        .await
        .unwrap();
    let file_b = files::ensure_file_at_path(&pool, proj_b.id, "tm/b.json")
        .await
        .unwrap();
    let file_c = files::ensure_file_at_path(&pool, proj_c_id, "tm/c.json")
        .await
        .unwrap();

    // 相同/相似源文：三个项目都用同一个英文原文，触发器提取 source_text
    let make_batch = |key: &str| {
        vec![entries::UploadEntry {
            key: key.to_string(),
            original: serde_json::json!({ "zh-Hans": "今天天气晴朗" }),
            context: None,
            translation: Some("the weather is sunny today".to_string()),
            state: Some("translated".to_string()),
        }]
    };

    let stats_a = entries::bulk_upsert(
        &pool,
        file_a.id,
        proj_a.id,
        &make_batch("tm_a1"),
        Some(u.id),
    )
    .await
    .unwrap();
    assert_eq!(stats_a.created, 1);

    let stats_b = entries::bulk_upsert(
        &pool,
        file_b.id,
        proj_b.id,
        &make_batch("tm_b1"),
        Some(u.id),
    )
    .await
    .unwrap();
    assert_eq!(stats_b.created, 1);

    let stats_c = entries::bulk_upsert(
        &pool,
        file_c.id,
        proj_c_id,
        &make_batch("tm_c1"),
        Some(u.id),
    )
    .await
    .unwrap();
    assert_eq!(stats_c.created, 1);

    // —— 取各词条真实 id ——
    let entry_a_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'tm_a1'")
            .bind(proj_a.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let entry_b_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'tm_b1'")
            .bind(proj_b.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let entry_c_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'tm_c1'")
            .bind(proj_c_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— 调用 suggestions_trgm，以 A 的词条为查询基点 ——
    let results =
        prts_db::search::suggestions_trgm(&pool, u.id, "en", "今天天气晴朗", entry_a_id, 0.0, 10)
            .await
            .unwrap();

    let result_ids: Vec<i64> = results.iter().map(|r| r.entry_id).collect();

    // B 的词条应出现（跨项目，U 是成员）
    assert!(
        result_ids.contains(&entry_b_id),
        "B 项目词条应出现在建议中（U 是成员），实际：{result_ids:?}"
    );

    // C 的词条不应出现（U 不是 C 的成员）
    assert!(
        !result_ids.contains(&entry_c_id),
        "C 项目词条不应出现在建议中（U 不是成员），实际：{result_ids:?}"
    );

    // A 的被查词条本身不应出现（cur_entry_id 排除）
    assert!(
        !result_ids.contains(&entry_a_id),
        "被查词条自身不应出现在建议结果中，实际：{result_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj_a.id).await.unwrap();
    projects::delete(&pool, proj_b.id).await.unwrap();
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(proj_c_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_tm_user'")
        .execute(&pool)
        .await
        .unwrap();
}

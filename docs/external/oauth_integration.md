# ZOOT OAuth2 接入文档

零协会登记系统（ZOOT）作为 OAuth2 授权服务器（Provider），支持第三方应用以「使用 ZOOT 账号登录」的方式接入。

本系统采用 **授权码模式（Authorization Code）+ PKCE**，签发 **JWT access_token**，并提供 `/userinfo` 端点按授权范围返回用户信息。

---

## PRTS 安装说明

PRTS 默认安装不编译或暴露 OAuth。需要 ZOOT 登录时，先在 `.env` 配置 `PRTS__AUTH__PUBLIC_BASE_URL`、`PRTS__AUTH__ZOOT__CLIENT_ID`、`PRTS__AUTH__ZOOT__CLIENT_SECRET` 及三个 ZOOT 端点，再使用可选 Compose 覆盖：

```bash
docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.oauth.yml \
  up -d
```

该组合使用带 `zoot-oauth` feature 的 `prts-backend:oauth-latest`。在 ZOOT 后台登记的 PRTS 回调地址为：

```text
<PRTS__AUTH__PUBLIC_BASE_URL>/api/auth/oauth/zoot/callback
```

只运行默认 `deploy/docker-compose.yml` 时，即使填写了 ZOOT 环境变量，也不会注册 OAuth 路由。`oauth-only` 运行时模式也只适用于已启用 `zoot-oauth` 的安装。

---

## 一、准备工作

1. 联系 ZOOT 管理员，在后台「OAuth 应用管理」中为你的应用创建一个客户端。
2. 创建时需提供：
   - **应用名称**、**应用描述**（展示在用户授权页）
   - **回调地址（redirect_uri）**：可填多个，每行一个；换取令牌时必须 **完全一致**（精确匹配，区分大小写、末尾斜杠、查询串）
   - **授权范围（scope）**：勾选应用需要的权限
3. 创建成功后你会拿到一次性展示的：
   - `client_id`
   - `client_secret`（**仅显示一次，请立即保存**；丢失需让管理员重置）

---

## 二、端点一览

| 端点 | 方法 | 说明 |
| --- | --- | --- |
| `/oauth/authorize` | GET | 引导用户授权（浏览器跳转） |
| `/oauth/token` | POST | 用授权码或刷新令牌换取访问令牌 |
| `/oauth/userinfo` | GET / POST | 凭 access_token 获取用户信息 |
| `/oauth/revoke` | POST | 撤销刷新令牌 |

> 以下示例中的 `https://zoot.example.com` 请替换为实际部署域名。

---

## 三、授权范围（Scope）

| scope | 含义 | `/userinfo` 返回字段 |
| --- | --- | --- |
| `profile` | 基础身份信息（**默认且始终包含**） | `username`、`role`、`picture` |
| `qq` | QQ 信息 | `qq_number`、`qq_nickname`、`qq_card` |
| `external` | 外部平台身份 | `paratranz_id`、`paratranz_username`、`paratranz_nickname`、`github_id`、`bilibili_uid`、`bilibili_nickname` |
| `work` | 工作信息 | `work_scope`、`work_content` |

说明：
- 多个 scope 用 **空格** 分隔，例如 `profile qq work`。
- `profile` 始终包含，即使未显式请求。
- 实际授予的 scope 会被收窄为「应用注册时允许的 scope」∩「本次请求的 scope」，并由用户在授权页确认。
- `/userinfo` 始终返回 `sub`（用户唯一 ID，字符串）。
- `picture` 为用户上传头像的完整 URL；用户未上传头像时返回 `null`。该字段沿用 OpenID Connect 标准 Claims 中 `picture` 表示头像 URL 的命名。

---

## 四、完整授权流程

```
用户浏览器                    你的应用                      ZOOT
    │                          │                            │
    │   点击「用 ZOOT 登录」    │                            │
    │─────────────────────────>│                            │
    │                          │ 生成 code_verifier/challenge│
    │   302 跳转到 /authorize   │                            │
    │<─────────────────────────│                            │
    │   GET /oauth/authorize ...（带 code_challenge）        │
    │──────────────────────────────────────────────────────>│
    │            登录（如未登录）+ 展示授权同意页              │
    │<──────────────────────────────────────────────────────│
    │                  用户点击「授权」                       │
    │──────────────────────────────────────────────────────>│
    │   302 跳转回 redirect_uri?code=...&state=...            │
    │<──────────────────────────────────────────────────────│
    │   GET redirect_uri?code=... │                          │
    │─────────────────────────>│                            │
    │                          │ POST /oauth/token（带 code +│
    │                          │ code_verifier + 客户端凭证） │
    │                          │───────────────────────────>│
    │                          │  access_token + refresh_token│
    │                          │<───────────────────────────│
    │                          │ GET /oauth/userinfo（Bearer）│
    │                          │───────────────────────────>│
    │                          │        用户信息 JSON         │
    │                          │<───────────────────────────│
```

### 第 1 步：生成 PKCE 参数

每次发起授权前，生成一个随机 `code_verifier`，并由它派生 `code_challenge`。

- `code_verifier`：43–128 个字符的随机串（推荐 `secrets.token_urlsafe(64)` 截断到合法长度）
- `code_challenge` = BASE64URL( SHA256( code_verifier ) )，去掉末尾 `=`
- `code_challenge_method` 固定为 **`S256`**（本系统不接受 `plain`）

```python
import hashlib, base64, secrets

code_verifier = secrets.token_urlsafe(64)[:96]   # 43-128 字符
digest = hashlib.sha256(code_verifier.encode('ascii')).digest()
code_challenge = base64.urlsafe_b64encode(digest).rstrip(b'=').decode('ascii')
```

将 `code_verifier` 暂存于服务端会话（与 `state` 绑定），换取令牌时使用。

### 第 2 步：跳转到授权端点

引导用户浏览器跳转到 `/oauth/authorize`：

```
GET https://zoot.example.com/oauth/authorize
    ?response_type=code
    &client_id=YOUR_CLIENT_ID
    &redirect_uri=https%3A%2F%2Fapp.example.com%2Fcallback
    &scope=profile%20qq
    &state=RANDOM_STATE
    &code_challenge=GENERATED_CHALLENGE
    &code_challenge_method=S256
```

查询参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `response_type` | 是 | 固定为 `code` |
| `client_id` | 是 | 你的客户端 ID |
| `redirect_uri` | 是 | 必须与注册值之一完全一致 |
| `scope` | 否 | 空格分隔；缺省为 `profile` |
| `state` | 推荐 | 防 CSRF 的随机串，回调时原样带回，需校验 |
| `code_challenge` | 是 | 第 1 步生成的 challenge |
| `code_challenge_method` | 是 | 固定为 `S256` |

行为：
- 用户未登录 → 跳转到 ZOOT 登录页，登录后自动回到本授权请求。
- 用户已登录 → 展示授权同意页，列出应用名称与申请的权限。
- 用户点击「授权」→ 浏览器 302 跳转到 `redirect_uri?code=...&state=...`。
- 用户点击「拒绝」→ 跳转到 `redirect_uri?error=access_denied&state=...`。

> 参数错误的处理：若 `client_id`/`redirect_uri` 无效，直接展示错误页（**不会**跳转，避免开放重定向）；其余错误（如 `response_type`、`code_challenge` 不合法）会以 `error=...` 跳转回 `redirect_uri`。

### 第 3 步：回调校验

在你的 `redirect_uri` 处理回调：

1. 校验 `state` 与发起时一致（不一致则拒绝）。
2. 若带 `error` 参数，按错误处理（用户拒绝等）。
3. 取出 `code`，进入第 4 步。

授权码为 **一次性**，有效期约 10 分钟，且一经在 `/token` 提交即作废（无论成功与否）。

### 第 4 步：用授权码换取令牌

```
POST https://zoot.example.com/oauth/token
Content-Type: application/x-www-form-urlencoded
```

```
grant_type=authorization_code
&code=AUTH_CODE
&redirect_uri=https%3A%2F%2Fapp.example.com%2Fcallback
&code_verifier=YOUR_CODE_VERIFIER
&client_id=YOUR_CLIENT_ID
&client_secret=YOUR_CLIENT_SECRET
```

客户端凭证支持两种传法（二选一）：
- **HTTP Basic 认证头**（推荐）：`Authorization: Basic base64(client_id:client_secret)`
- 表单参数 `client_id` + `client_secret`（如上例）

成功响应（`200`）：

```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIs...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "refresh_token": "x7Jk...",
  "scope": "profile qq"
}
```

- `access_token`：JWT，默认 1 小时有效（`expires_in` 秒）。
- `refresh_token`：不透明字符串，默认 30 天有效，用于续期。

### 第 5 步：获取用户信息

```
GET https://zoot.example.com/oauth/userinfo
Authorization: Bearer ACCESS_TOKEN
```

成功响应（`200`），字段随授权 scope 变化：

```json
{
  "sub": "42",
  "username": "alice",
  "role": "member",
  "picture": "https://zoot.example.com/static/uploads/avatars/user_42_xxx.png",
  "qq_number": 10001,
  "qq_nickname": "Alice",
  "qq_card": "翻译-Alice"
}
```

---

## 五、刷新与撤销

### 刷新访问令牌

`access_token` 过期后，用 `refresh_token` 换取新的 `access_token`：

```
POST https://zoot.example.com/oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=refresh_token
&refresh_token=YOUR_REFRESH_TOKEN
&client_id=YOUR_CLIENT_ID
&client_secret=YOUR_CLIENT_SECRET
```

可选携带 `scope` 参数以 **收窄**（不可扩大）本次签发令牌的范围。

成功响应（`200`）：

```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIs...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "profile qq"
}
```

### 撤销刷新令牌

用户登出或解绑时，撤销 `refresh_token`（遵循 RFC 7009，幂等，令牌不存在也返回 `200`）：

```
POST https://zoot.example.com/oauth/revoke
Content-Type: application/x-www-form-urlencoded

token=YOUR_REFRESH_TOKEN
&client_id=YOUR_CLIENT_ID
&client_secret=YOUR_CLIENT_SECRET
```

---

## 六、错误码

`/token`、`/revoke`、`/userinfo` 的错误遵循 OAuth2 风格，返回 JSON：

```json
{ "error": "invalid_grant", "error_description": "授权码已过期" }
```

| HTTP | error | 常见原因 |
| --- | --- | --- |
| 400 | `invalid_request` | 缺少必填参数 |
| 400 | `invalid_grant` | 授权码无效/已用/过期、PKCE 校验失败、redirect_uri 不匹配、refresh_token 无效/已撤销/过期 |
| 400 | `invalid_scope` | 刷新时请求的 scope 超出原授权范围 |
| 400 | `unsupported_grant_type` | `grant_type` 不是 `authorization_code` 或 `refresh_token` |
| 401 | `invalid_client` | 客户端凭证缺失或错误 |
| 401 | `invalid_token` | `/userinfo` 的 Bearer 令牌缺失/无效/过期 |
| 500 | `server_error` | 服务端异常 |

---

## 七、安全注意事项

- 全程使用 **HTTPS**。
- `client_secret` 仅保存在你的服务端，**切勿** 下发到浏览器或移动端。
- 每次授权都必须使用新的 `code_verifier` / `state`，并在回调时严格校验 `state`。
- `code_challenge_method` 必须是 `S256`；本系统拒绝 `plain`。
- `access_token`（JWT）为自包含令牌，过期前无法主动吊销；如需即时失效，请缩短使用场景或依赖 `refresh_token` 的撤销。
- `redirect_uri` 为精确匹配，调试时注意端口、末尾斜杠、查询串的差异。

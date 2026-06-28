# PRTS 后端镜像（多阶段构建）。构建上下文为仓库根目录。
# 提示：后续可引入 cargo-chef 进一步缓存依赖层，此处保持简单以求正确。

# ---- 构建阶段 ----
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY backend ./backend
RUN cargo build --release --manifest-path backend/Cargo.toml --bin prts-api

# ---- 运行阶段 ----
FROM debian:bookworm-slim AS runtime
WORKDIR /app
# libssl3：reqwest 用 native-tls，Linux 上动态链接 OpenSSL（ZOOT OAuth 调用需要）。
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -r -u 10001 prts
# 二进制 + 默认配置（迁移已在编译期嵌入二进制）
COPY --from=builder /app/backend/target/release/prts-api /usr/local/bin/prts-api
COPY backend/config ./config
USER prts
EXPOSE 3000
# 容器健康检查：命中存活探针
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=5 \
  CMD curl -fsS http://localhost:3000/health || exit 1
CMD ["prts-api"]

# PRTS 前端镜像：Node 构建静态资源 → nginx 托管。构建上下文为仓库根目录。

# ---- 构建阶段 ----
FROM node:22-alpine AS builder
WORKDIR /app/frontend
# 显式安装与 lockfile 一致的 pnpm 版本（避免 corepack 取到带 minimumReleaseAge 策略的新版而拒装）
RUN npm install -g pnpm@10.6.3
# 先装依赖以利用缓存
COPY frontend/package.json frontend/pnpm-lock.yaml* ./
RUN pnpm install --frozen-lockfile
COPY frontend ./
RUN pnpm build

# ---- 运行阶段 ----
FROM nginx:alpine AS runtime
COPY deploy/nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/frontend/dist /usr/share/nginx/html
EXPOSE 80

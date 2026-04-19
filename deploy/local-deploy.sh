#!/usr/bin/env sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

APP_IMAGE="${APP_IMAGE:-prts-backend-local}"
WEB_IMAGE="${WEB_IMAGE:-prts-web-local}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
EDGE_PORT="${EDGE_PORT:-18000}"
COMPOSE_FILE="${COMPOSE_FILE:-$REPO_ROOT/deploy/docker-compose.prod.yml}"
GOPROXY_VALUE="${GOPROXY_VALUE:-https://goproxy.cn,direct}"
GOSUMDB_VALUE="${GOSUMDB_VALUE:-off}"
HTTP_PROXY_VALUE="${HTTP_PROXY_VALUE:-}"
HTTPS_PROXY_VALUE="${HTTPS_PROXY_VALUE:-}"
NO_PROXY_VALUE="${NO_PROXY_VALUE:-}"
NPM_REGISTRY_VALUE="${NPM_REGISTRY_VALUE:-https://registry.npmmirror.com}"

cd "$REPO_ROOT"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker 未安装或不在 PATH 中"
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "当前用户无权访问 Docker，请先修复 docker 权限"
  exit 1
fi

if [ ! -f ".env" ]; then
  echo ".env 不存在，请先复制 .env.example 为 .env 并修改配置"
  exit 1
fi

printf "后端镜像名 [%s]: " "$APP_IMAGE"
read -r input_app_image || true
if [ -n "${input_app_image:-}" ]; then
  APP_IMAGE="$input_app_image"
fi

printf "前端镜像名 [%s]: " "$WEB_IMAGE"
read -r input_web_image || true
if [ -n "${input_web_image:-}" ]; then
  WEB_IMAGE="$input_web_image"
fi

printf "对外监听端口 [%s]: " "$EDGE_PORT"
read -r input_edge_port || true
if [ -n "${input_edge_port:-}" ]; then
  EDGE_PORT="$input_edge_port"
fi

printf "Go 模块代理 GOPROXY [%s]: " "$GOPROXY_VALUE"
read -r input_goproxy || true
if [ -n "${input_goproxy:-}" ]; then
  GOPROXY_VALUE="$input_goproxy"
fi

printf "Go 校验服务 GOSUMDB [%s]: " "$GOSUMDB_VALUE"
read -r input_gosumdb || true
if [ -n "${input_gosumdb:-}" ]; then
  GOSUMDB_VALUE="$input_gosumdb"
fi

printf "HTTP_PROXY [%s]: " "${HTTP_PROXY_VALUE:-<empty>}"
read -r input_http_proxy || true
if [ -n "${input_http_proxy:-}" ]; then
  HTTP_PROXY_VALUE="$input_http_proxy"
fi

printf "HTTPS_PROXY [%s]: " "${HTTPS_PROXY_VALUE:-<empty>}"
read -r input_https_proxy || true
if [ -n "${input_https_proxy:-}" ]; then
  HTTPS_PROXY_VALUE="$input_https_proxy"
fi

printf "NO_PROXY [%s]: " "${NO_PROXY_VALUE:-<empty>}"
read -r input_no_proxy || true
if [ -n "${input_no_proxy:-}" ]; then
  NO_PROXY_VALUE="$input_no_proxy"
fi

printf "NPM registry [%s]: " "${NPM_REGISTRY_VALUE:-<empty>}"
read -r input_npm_registry || true
if [ -n "${input_npm_registry:-}" ]; then
  NPM_REGISTRY_VALUE="$input_npm_registry"
fi

printf "是否重新构建后端镜像？[Y/n]: "
read -r rebuild_backend || true
rebuild_backend=${rebuild_backend:-Y}

printf "是否重新构建前端镜像？[Y/n]: "
read -r rebuild_web || true
rebuild_web=${rebuild_web:-Y}

if [ "$rebuild_backend" = "Y" ] || [ "$rebuild_backend" = "y" ]; then
  docker build \
    --build-arg GOPROXY="$GOPROXY_VALUE" \
    --build-arg GOSUMDB="$GOSUMDB_VALUE" \
    --build-arg HTTP_PROXY="$HTTP_PROXY_VALUE" \
    --build-arg HTTPS_PROXY="$HTTPS_PROXY_VALUE" \
    --build-arg NO_PROXY="$NO_PROXY_VALUE" \
    -t "${APP_IMAGE}:${IMAGE_TAG}" .
fi

if [ "$rebuild_web" = "Y" ] || [ "$rebuild_web" = "y" ]; then
  docker build \
    --build-arg HTTP_PROXY="$HTTP_PROXY_VALUE" \
    --build-arg HTTPS_PROXY="$HTTPS_PROXY_VALUE" \
    --build-arg NO_PROXY="$NO_PROXY_VALUE" \
    --build-arg NPM_REGISTRY="$NPM_REGISTRY_VALUE" \
    -t "${WEB_IMAGE}:${IMAGE_TAG}" ./web
fi

export APP_IMAGE
export WEB_IMAGE
export APP_IMAGE_TAG="$IMAGE_TAG"
export EDGE_PORT

docker compose -f "$COMPOSE_FILE" up -d postgres redis
docker compose -f "$COMPOSE_FILE" run --rm api prts-migrate
docker compose -f "$COMPOSE_FILE" up -d --remove-orphans

echo ""
echo "部署完成"
echo "访问地址: http://<服务器IP>:${EDGE_PORT}"

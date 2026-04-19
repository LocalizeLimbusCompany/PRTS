#!/usr/bin/env sh
set -eu

APP_IMAGE="${APP_IMAGE:-prts-backend-local}"
WEB_IMAGE="${WEB_IMAGE:-prts-web-local}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
EDGE_PORT="${EDGE_PORT:-18000}"
COMPOSE_FILE="${COMPOSE_FILE:-deploy/docker-compose.prod.yml}"

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

printf "是否重新构建后端镜像？[Y/n]: "
read -r rebuild_backend || true
rebuild_backend=${rebuild_backend:-Y}

printf "是否重新构建前端镜像？[Y/n]: "
read -r rebuild_web || true
rebuild_web=${rebuild_web:-Y}

if [ "$rebuild_backend" = "Y" ] || [ "$rebuild_backend" = "y" ]; then
  docker build -t "${APP_IMAGE}:${IMAGE_TAG}" .
fi

if [ "$rebuild_web" = "Y" ] || [ "$rebuild_web" = "y" ]; then
  docker build -t "${WEB_IMAGE}:${IMAGE_TAG}" ./web
fi

export APP_IMAGE
export WEB_IMAGE
export APP_IMAGE_TAG="$IMAGE_TAG"
export EDGE_PORT

docker compose -f "$COMPOSE_FILE" run --rm api prts-migrate
docker compose -f "$COMPOSE_FILE" up -d --remove-orphans

echo ""
echo "部署完成"
echo "访问地址: http://<服务器IP>:${EDGE_PORT}"

#!/usr/bin/env sh
set -eu

APP_DIR="${APP_DIR:-/opt/prts-translation-system}"
APP_IMAGE="${APP_IMAGE:-ghcr.io/example/prts-translation-system}"
WEB_IMAGE="${WEB_IMAGE:-ghcr.io/example/prts-translation-system-web}"
IMAGE_TAG="${IMAGE_TAG:-latest}"

cd "$APP_DIR"
export APP_IMAGE
export WEB_IMAGE
export APP_IMAGE_TAG="$IMAGE_TAG"

DOCKER_BIN="docker"
if ! docker info >/dev/null 2>&1; then
  DOCKER_BIN="sudo docker"
fi

sh -c "$DOCKER_BIN compose -f docker-compose.prod.yml pull"
sh -c "$DOCKER_BIN compose -f docker-compose.prod.yml run --rm api prts-migrate"
sh -c "$DOCKER_BIN compose -f docker-compose.prod.yml up -d --remove-orphans"
sh -c "$DOCKER_BIN image prune -f"

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

docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml run --rm api prts-migrate
docker compose -f docker-compose.prod.yml up -d --remove-orphans
docker image prune -f

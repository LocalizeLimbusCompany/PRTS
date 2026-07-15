#!/bin/sh
set -eu

# Docker named volumes are initially mounted as root-owned directories. Prepare only the
# configured volume roots, then permanently drop privileges before starting any PRTS command.
media_directory=${PRTS__MEDIA__DIRECTORY:-/app/data/media}
upload_temp_directory=${PRTS__MEDIA__UPLOAD_TEMP_DIRECTORY:-/app/data/upload-temp}

for directory in "$media_directory" "$upload_temp_directory"; do
  mkdir -p "$directory"
  chown prts:prts "$directory"
  chmod u+rwx "$directory"
done

exec setpriv \
  --reuid="$(id -u prts)" \
  --regid="$(id -g prts)" \
  --init-groups \
  "$@"

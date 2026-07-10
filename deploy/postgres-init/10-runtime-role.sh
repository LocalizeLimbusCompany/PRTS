#!/usr/bin/env bash
set -euo pipefail

: "${POSTGRES_RUNTIME_USER:?POSTGRES_RUNTIME_USER must be set}"
: "${POSTGRES_RUNTIME_PASSWORD:?POSTGRES_RUNTIME_PASSWORD must be set}"

if [[ "${POSTGRES_RUNTIME_USER}" == "${POSTGRES_USER}" ]]; then
  echo "runtime role must differ from migration owner" >&2
  exit 1
fi

# psql variables are rendered with format(%I/%L); neither role nor password is concatenated as SQL.
psql --set=ON_ERROR_STOP=1 \
  --username "${POSTGRES_USER}" \
  --dbname "${POSTGRES_DB}" \
  --set=runtime_user="${POSTGRES_RUNTIME_USER}" \
  --set=runtime_password="${POSTGRES_RUNTIME_PASSWORD}" <<'SQL'
SELECT format(
  'CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L',
  :'runtime_user',
  :'runtime_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'runtime_user')
\gexec
SQL

#!/bin/sh
# Creates the `pp` tenant through Supavisor's API, then stays up so
# `docker compose up --wait` can tell provisioning finished.
set -eu
# Token from Supavisor's docs, signed with API_JWT_SECRET=dev.
token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJvbGUiOiJhbm9uIiwiaWF0IjoxNjQ1MTkyODI0LCJleHAiOjE5NjA3Njg4MjR9.M9jrxyvPLkUxWgOYSf5dNdJ8v_eRrq810ShFRT8N-6M
until curl -sf http://supavisor:4000/api/health >/dev/null; do sleep 1; done
curl -sS --fail-with-body -X PUT http://supavisor:4000/api/tenants/pp \
  -H "Authorization: Bearer $token" -H "Content-Type: application/json" \
  --data @/tenant.json
touch /tmp/done
exec sleep infinity

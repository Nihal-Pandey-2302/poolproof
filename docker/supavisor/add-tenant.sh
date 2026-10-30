#!/bin/sh
# Creates tenants through Supavisor's API, then stays up so `docker compose up --wait`
# can tell provisioning finished. TENANTS is a list of id:file pairs.
set -eu
# Token from Supavisor's docs, signed with API_JWT_SECRET=dev.
token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJvbGUiOiJhbm9uIiwiaWF0IjoxNjQ1MTkyODI0LCJleHAiOjE5NjA3Njg4MjR9.M9jrxyvPLkUxWgOYSf5dNdJ8v_eRrq810ShFRT8N-6M
until curl -sf "http://$SUPAVISOR/api/health" >/dev/null; do sleep 1; done
for pair in $TENANTS; do
  curl -sS --fail-with-body -X PUT "http://$SUPAVISOR/api/tenants/${pair%%:*}" \
    -H "Authorization: Bearer $token" -H "Content-Type: application/json" \
    --data "@/tenants/${pair#*:}"
  echo
done
touch /tmp/done
exec sleep infinity

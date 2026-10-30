#!/bin/sh
# One metadata database per Supavisor instance, each with the schema Supavisor uses.
set -eu
for db in supavisor_2_9 supavisor_2_10; do
  psql -v ON_ERROR_STOP=1 -U postgres -c "CREATE DATABASE $db"
  psql -v ON_ERROR_STOP=1 -U postgres -d "$db" -c "CREATE SCHEMA _supavisor"
done

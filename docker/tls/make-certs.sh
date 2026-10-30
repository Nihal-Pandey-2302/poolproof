#!/bin/sh
# One-shot: a throwaway CA and a server certificate for the test bed's TLS targets,
# kept in the `tls` volume. Only ca.pem goes to /out (docker/tls/out on the host,
# git-ignored), for clients to trust. Postgres and PgBouncer run as different users,
# so each gets its own key copy.
set -eu
cd /certs
if [ ! -f ca.pem ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=poolproof test CA" \
    -keyout ca.key -out ca.pem 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" \
    -keyout server.key -out server.csr 2>/dev/null
  printf 'subjectAltName=DNS:localhost,DNS:postgres,IP:127.0.0.1\n' > san.ext
  openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -days 3650 \
    -extfile san.ext -out server.crt 2>/dev/null
  rm server.csr san.ext
fi
install -m 600 -o 999 -g 999 server.key postgres.key
install -m 600 -o 70 -g 70 server.key pgbouncer.key
chmod 644 ca.pem server.crt
cp ca.pem /out/ca.pem
# Writable for the host user, so the files can be removed without root.
chmod 666 /out/ca.pem
chmod 777 /out

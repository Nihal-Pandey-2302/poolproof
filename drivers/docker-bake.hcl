# Builds the driver images `poolproof drivers` runs:
#   docker buildx bake -f drivers/docker-bake.hcl --load
# CI sets CACHE=gha to keep build layers in the GitHub Actions cache, one scope per driver.

variable "CACHE" {
  default = ""
}

group "default" {
  targets = ["psycopg", "pgx", "node-postgres", "pgjdbc"]
}

target "driver" {
  name       = d
  matrix     = { d = ["psycopg", "pgx", "node-postgres", "pgjdbc"] }
  context    = "drivers/${d}"
  tags       = ["poolproof-driver-${d}:dev"]
  cache-from = CACHE == "gha" ? ["type=gha,scope=driver-${d}"] : []
  cache-to   = CACHE == "gha" ? ["type=gha,scope=driver-${d},mode=max"] : []
}

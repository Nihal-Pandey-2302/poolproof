# Self-test proxy for the docker test bed (compose profile "selftest").
FROM rust:1.88-slim-bookworm AS build
WORKDIR /src
COPY . .
# A debug build compiles faster, and speed doesn't matter for the self-test.
# JOBS limits parallel compile jobs (e.g. --build-arg JOBS=2 on a busy machine).
ARG JOBS
RUN cargo build --locked --bin pp-proxy ${JOBS:+--jobs $JOBS}

FROM debian:bookworm-slim
COPY --from=build /src/target/debug/pp-proxy /usr/local/bin/pp-proxy
USER 65534
ENTRYPOINT ["pp-proxy"]

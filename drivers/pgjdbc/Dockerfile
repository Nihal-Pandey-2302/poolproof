# poolproof's pgjdbc program. Build with drivers/docker-bake.hcl.
FROM maven:3.9-eclipse-temurin-21@sha256:99e61abcff91a9b1333463bd8451fb18495d6eba9250ac66a338b518f8278320 AS build
WORKDIR /src
COPY pom.xml ./
# --strict-checksums: fail if a download doesn't match Maven Central's checksum.
RUN mvn -B -q --strict-checksums dependency:copy-dependencies -DoutputDirectory=/app/lib -DincludeScope=runtime
COPY Smoke.java ./
RUN javac -cp "/app/lib/*" -d /app/classes Smoke.java

FROM eclipse-temurin:21-jre@sha256:0b99b3376fbb50dbf9c0af8535fdd5b8eb0d3fdfc8b1e4967f383b6919f6901a
COPY --from=build /app /app
USER nobody
# A short-lived program: skip the optimizing compiler, start faster.
ENTRYPOINT ["java", "-XX:TieredStopAtLevel=1", "-XX:+UseSerialGC", "-cp", "/app/classes:/app/lib/*", "Smoke"]

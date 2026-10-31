# poolproof's pgjdbc program. Build with drivers/docker-bake.hcl.
FROM maven:3.9-eclipse-temurin-26@sha256:b2c1ad85954592f9928e84327c65201f308ad9b5d8ed7d5b823717c97bf23fbb AS build
WORKDIR /src
COPY pom.xml ./
# --strict-checksums: fail if a download doesn't match Maven Central's checksum.
RUN mvn -B -q --strict-checksums dependency:copy-dependencies -DoutputDirectory=/app/lib -DincludeScope=runtime
COPY Smoke.java ./
# --release 21: class files the runtime below can load, whatever JDK compiles them.
RUN javac --release 21 -cp "/app/lib/*" -d /app/classes Smoke.java

FROM eclipse-temurin:21-jre@sha256:0b99b3376fbb50dbf9c0af8535fdd5b8eb0d3fdfc8b1e4967f383b6919f6901a
COPY --from=build /app /app
USER nobody
# A short-lived program: skip the optimizing compiler, start faster.
ENTRYPOINT ["java", "-XX:TieredStopAtLevel=1", "-XX:+UseSerialGC", "-cp", "/app/classes:/app/lib/*", "Smoke"]

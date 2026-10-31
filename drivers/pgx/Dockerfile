# poolproof's pgx program. Build with drivers/docker-bake.hcl.
FROM golang:1.25-bookworm@sha256:3b4a11519ad929d1e1d261a12cff056f0c85b735253d7d861346b9c6f8b36437 AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY main.go ./
# -mod=readonly: exactly go.mod and go.sum, and fail if they'd need changing.
RUN CGO_ENABLED=0 go build -mod=readonly -trimpath -o /smoke .

FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
COPY --from=build /smoke /smoke
ENTRYPOINT ["/smoke"]

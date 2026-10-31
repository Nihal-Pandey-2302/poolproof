# poolproof's psycopg program. Build with drivers/docker-bake.hcl.
FROM python:3.15-slim-bookworm@sha256:4e03260adeeebafccd084f3584e6b84823aba5651cfb53ef8a078ed441ddf9cd
COPY --from=ghcr.io/astral-sh/uv:0.12.5@sha256:e85be844203885286c60ffad8a858d48afb6c5a5c237ca0e67f12e74b8f174b1 /uv /usr/local/bin/uv
ENV UV_PYTHON_DOWNLOADS=never UV_LINK_MODE=copy
WORKDIR /app
COPY pyproject.toml uv.lock ./
# --frozen: exactly the lockfile, and fail if it doesn't match pyproject.toml.
RUN uv sync --frozen --no-dev
COPY smoke.py ./
USER nobody
ENTRYPOINT ["/app/.venv/bin/python", "-u", "smoke.py"]

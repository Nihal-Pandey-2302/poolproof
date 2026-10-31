# poolproof's node-postgres program. Build with drivers/docker-bake.hcl.
FROM node:22-bookworm-slim@sha256:4b150b48b4116e2a22f2c39cc27025fbf4f153e2078fd3b14d159fdb5c7279da
WORKDIR /app
COPY package.json package-lock.json ./
# npm ci: exactly the lockfile, and fail if it doesn't match package.json.
RUN npm ci --omit=dev --ignore-scripts && npm cache clean --force
COPY smoke.js ./
USER node
ENTRYPOINT ["node", "smoke.js"]

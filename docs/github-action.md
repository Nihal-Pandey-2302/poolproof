# GitHub Action

The repository is also an action. It downloads the release binary for the runner, checks
it against the release's SHA256SUMS and its build attestation (`gh attestation verify`,
signed by `release.yml`), then runs `scan` or `run`. Pin a release: the action uses the
same version of poolproof as the tag in `uses:`. Pinned by commit SHA? Set `version:` too
(e.g. `version: v0.1.0`); the action can't tell the release from a commit and stops.

```yaml
permissions:
  contents: read
  security-events: write   # only for the SARIF upload

steps:
  - uses: actions/checkout@v7
  - uses: Nihal-Pandey-2302/poolproof@v0.1.0
    with:
      command: scan
      args: migrations/all.sql
      sarif: true            # findings also show up in code scanning
```

`run` needs Postgres and your pooler running first (service containers, or
`docker compose up --wait`). The results table goes to the job summary:

```yaml
  - uses: Nihal-Pandey-2302/poolproof@v0.1.0
    with:
      command: run
      args: |
        --targets
        ci/poolproof-targets.toml
        --fail-on
        silent
```

`args` takes one argument per line, as written: no shell quoting and no variables. Leave
out `--format`; the action sets it. The step fails with poolproof's exit code, after the
SARIF upload and the summary. `summary: false` skips the job summary, and
`upload-sarif: false` writes the SARIF without uploading it. Outputs: `exit-code`,
`report` (the JSON from `run`) and `sarif`. `version:` picks another release (`latest`
works, with a warning); `binary:` uses a poolproof you built, unverified. Runners: Linux
x86_64 and arm64, macOS x86_64 and arm64, Windows x86_64, with the GitHub CLI installed
(it is on GitHub's own).

# Releasing

## Once (repository admin)

These are repository and crates.io settings, so only a repository admin can do them.
Steps 1 and 2 come before the first release. v0.1.0 was released on GitHub only, without
crates.io, so steps 3 to 8 still wait for the first release that goes to crates.io.

1. Done: the repository is public, and GitHub Pages is enabled with GitHub Actions as
   the source. The `pages` job in `ci.yml` deploys the results site on every push to
   main. Artifact attestations (below) also need the public repository on the free plan.
2. Enable private vulnerability reporting: Settings > Code security > Private
   vulnerability reporting > Enable. SECURITY.md points reporters there.
3. Create the `crates-io` environment: Settings > Environments > New environment.
   Add yourself as a required reviewer and limit deployments to tags matching `v*`. The
   `publish` job in `release.yml` runs in this environment, so every crates.io upload
   waits for your approval.
4. In the release commit that step 5 publishes, add `cargo install --locked poolproof`
   and `cargo binstall poolproof` to the Install sections of README.md and
   docs/install.md, and say it is on crates.io. The crate's README on crates.io is the one
   in the published commit.
   `cargo binstall` finds the release archives through the crate's metadata, so it needs
   the crate on crates.io too.
5. Publish the first version by hand. Trusted publishing can only be set up for a crate
   that already exists:

   ```sh
   cargo publish --dry-run
   cargo publish          # with a crates.io API token from https://crates.io/settings/tokens
   ```

6. Set up trusted publishing on crates.io: crate page > Settings > Trusted Publishing >
   Add, with
   - repository owner: `Nihal-Pandey-2302`
   - repository name: `poolproof`
   - workflow filename: `release.yml`
   - environment: `crates-io`
7. Turn on the publish job: Settings > Secrets and variables > Actions > Variables >
   New repository variable `CRATES_IO_TRUSTED_PUBLISHING` = `true`.
8. Revoke the API token used in step 5. No crates.io token is stored in the repository.

Until step 7 is done, the publish job in `release.yml` is skipped (it runs only when
`CRATES_IO_TRUSTED_PUBLISHING` is `true`), so a tag builds the binaries, attests them and
creates the GitHub release, nothing more.

## Every release

1. Move the `Unreleased` entries in CHANGELOG.md under the new version and date. Pick the
   version by the policy in CHANGELOG.md. Change the `--tag` in the install command in
   README.md and docs/install.md to the new version.
2. Bump `version` in Cargo.toml, run `cargo check` so Cargo.lock follows, commit
   "Release vX.Y.Z", push, and wait for CI to be green on main.
3. Tag and push the tag:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

4. `release.yml` then builds the binaries, attests them, creates the GitHub release with
   SHA256SUMS, and (once step 7 above is done) publishes the crate after you approve the
   `crates-io` deployment. The publish job fails if the tag and Cargo.toml disagree.
5. Check the result:

   ```sh
   gh release download vX.Y.Z --repo Nihal-Pandey-2302/poolproof -p 'poolproof-x86_64-unknown-linux-gnu.tar.gz'
   gh attestation verify poolproof-x86_64-unknown-linux-gnu.tar.gz --repo Nihal-Pandey-2302/poolproof
   cargo binstall poolproof   # once the crate is on crates.io
   ```

   And the action's download path, which CI can't test before a release exists: in any
   repository, a workflow with `uses: Nihal-Pandey-2302/poolproof@vX.Y.Z` and
   `command: scan` must log the attestation check and poolproof's version. After the
   first release, add a scheduled job to `ci.yml` that runs the action that way against
   the latest release, so a broken download or verification shows up on its own.

Running `release.yml` by hand (Actions > release > Run workflow) only builds, which is
the way to check the build matrix without releasing.

## Later: a Homebrew tap (repository admin)

A tap is a separate public repository named `homebrew-poolproof`; then
`brew install Nihal-Pandey-2302/poolproof/poolproof` works. It needs one file,
`Formula/poolproof.rb`, updated for every release with the new version and the archives'
SHA-256 sums from SHA256SUMS:

```ruby
class Poolproof < Formula
  desc "Find out what breaks when you put a connection pooler in front of Postgres"
  homepage "https://github.com/Nihal-Pandey-2302/poolproof"
  version "X.Y.Z"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/Nihal-Pandey-2302/poolproof/releases/download/vX.Y.Z/poolproof-aarch64-apple-darwin.tar.gz"
      sha256 "..."
    end
    on_intel do
      url "https://github.com/Nihal-Pandey-2302/poolproof/releases/download/vX.Y.Z/poolproof-x86_64-apple-darwin.tar.gz"
      sha256 "..."
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/Nihal-Pandey-2302/poolproof/releases/download/vX.Y.Z/poolproof-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "..."
    end
    on_intel do
      url "https://github.com/Nihal-Pandey-2302/poolproof/releases/download/vX.Y.Z/poolproof-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "..."
    end
  end

  def install
    bin.install "poolproof"
    man1.install Dir["man/*.1"]
    bash_completion.install "completions/poolproof.bash" => "poolproof"
    zsh_completion.install "completions/_poolproof"
    fish_completion.install "completions/poolproof.fish"
  end

  test do
    assert_match "multi-statement-begin", shell_output("#{bin}/poolproof list")
  end
end
```

Homebrew unpacks the archive's single top-level directory, so the paths in `install` are
relative to `poolproof-<target>/`.

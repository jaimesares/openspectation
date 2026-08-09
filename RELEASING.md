# Releasing

Releases are automated from the second one onward. The first release
bootstraps the accounts and settings the automation depends on, so it is
manual by design.

## One-time setup (0.1.0)

1. Create the `jaimesares/openspectation` repository on GitHub and push:

   ```bash
   git remote add origin git@github.com:jaimesares/openspectation.git
   git push -u origin main
   ```

2. Wait for the `CI` workflow to pass on `main` — it runs the same
   `cargo publish --dry-run` gate used below.

3. Publish 0.1.0 from this machine. crates.io has no pending-publisher
   concept, so a trusted publisher can only be attached to a crate that
   already exists and the first upload has to use a regular token:

   ```bash
   cargo login    # token from https://crates.io/settings/tokens
   cargo publish
   ```

4. Configure Trusted Publishing at
   `https://crates.io/crates/openspectation/settings`: repository owner
   `jaimesares`, repository name `openspectation`, workflow filename
   `release.yml`, environment left empty. Revoke the token from step 3
   afterwards; nothing else uses it.

5. Tag the published commit so the GitHub Release with prebuilt binaries
   exists for 0.1.0 too:

   ```bash
   git tag -a v0.1.0 -m "openspectation 0.1.0"
   git push origin v0.1.0
   ```

   The run's `publish` job sees 0.1.0 already on crates.io and skips the
   upload, so the whole workflow still passes.

## Every release after

1. Bump `version` in `Cargo.toml`, move the `[Unreleased]` notes in
   `CHANGELOG.md` under the new version, commit.
2. Tag and push:

   ```bash
   git tag vX.Y.Z
   git push origin main vX.Y.Z
   ```

3. The `Release` workflow builds all eight targets, attaches the archives to
   the GitHub Release and publishes to crates.io through Trusted Publishing —
   an OIDC token minted per run, no stored secret.

A `workflow_dispatch` run of the same workflow builds every target and skips
both publishing jobs — use it to prove the matrix before tagging.

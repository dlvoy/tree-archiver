# Building and releasing

Tree Archiver is a Windows-only Tauri app: a Rust binary hosting a WebView2
control that renders a React frontend. This covers building it locally and the
GitHub Actions pipeline that publishes releases.

## What a build produces

| Artifact | Path after a local build |
| --- | --- |
| Application | `src-tauri/target/release/tree-archiver.exe` |
| NSIS installer | `src-tauri/target/release/bundle/nsis/Tree Archiver_<version>_x64-setup.exe` |
| MSI package | `src-tauri/target/release/bundle/msi/Tree Archiver_<version>_x64_en-US.msi` |

Both bundles are declared in `src-tauri/tauri.conf.json` under `bundle.targets`.
Tauri downloads NSIS and WiX on first use and caches them, so the first build on
a clean machine is slower than later ones.

The application is self-contained apart from the WebView2 runtime, which ships
with Windows 10 and 11. `tree-archiver.exe` runs on its own with nothing
installed, which is why the release publishes it as a portable download.

## Local builds

### Prerequisites

- **Rust**, MSVC toolchain — `rustup default stable-x86_64-pc-windows-msvc`.
  The crate sets `rust-version = "1.93"`, which is what the 7z encoder
  (`sevenz-rust2`) requires.
- **Visual Studio Build Tools** with the *Desktop development with C++* workload.
  This supplies `link.exe`; without it the Rust link step fails.
- **Node.js 20.19+ or 22.12+** (Vite 7's floor). CI uses 22.
- **WebView2 runtime** — preinstalled on Windows 10/11. Only worth checking if
  the window opens blank.

### Commands

```bash
npm install            # once
npm run tauri dev      # run with hot reload
npm run tauri build    # production build, both installers
```

Tests are Rust-side and run without building the frontend:

```bash
cd src-tauri
cargo test             # 128 tests: 119 unit, 9 end-to-end
```

The end-to-end suite writes real trees under `%TEMP%`, including a directory
junction and paths past 260 characters, and cleans up after itself.

Type-checking the frontend happens automatically during `npm run tauri build`,
since `beforeBuildCommand` runs `tsc --noEmit && vite build`. To check without a
full build, run `npx tsc --noEmit`.

## The release pipeline

`.github/workflows/release.yml` runs on `windows-latest` and **publishes only
for version tags**. Pushing to a branch does not build or publish anything.

```
on:
  push:
    tags: ["v*.*.*"]
  workflow_dispatch:
```

What it does, in order:

1. **Works out the version** from the tag (`v1.2.3` → `1.2.3`). A tag containing
   a hyphen — `v1.2.0-rc.1` — is published as a pre-release.
2. **Checks the tag against the manifests.** The version must match in all three
   of `src-tauri/tauri.conf.json`, `package.json`, and `src-tauri/Cargo.toml`,
   or the run fails immediately with an annotation naming the offender. This
   guard exists because installer file names come from `tauri.conf.json`, not
   from the tag: without it, tagging `v1.2.3` while the config still says
   `1.2.2` produces a release full of mislabelled files.
3. **Runs `cargo test --locked`.** A failing test stops the release.
4. **Clears `target/release/bundle`, then builds** with `npm run tauri build`.
   The bundle directory is wiped first because `target/` is restored from cache
   and may still hold installers from an earlier version.
5. **Collects artifacts** into `release/`, renaming them. The bundler's own
   names contain a space (`Tree Archiver_1.0.0_x64-setup.exe`), which becomes
   `%20` in a download URL, so they are renamed to hyphenated equivalents and a
   `checksums.txt` is generated. The globs that find the bundles are scoped to
   the version being built and the step fails unless each matches exactly one
   file — an open `*-setup.exe` would quietly pick up a leftover build.
6. **Assembles release notes** by lifting this version's section out of
   `CHANGELOG.md` (see `.github/scripts/release-notes.sh`) and appending a
   downloads table. A version with no changelog section still releases, with a
   placeholder body and a warning in the log.
7. **Uploads a build artifact** — always, including manual runs.
8. **Publishes the release** — tag runs only.

### Published assets

| Asset | Contents |
| --- | --- |
| `TreeArchiver-<version>-x64-setup.exe` | NSIS installer |
| `TreeArchiver-<version>-x64-portable.exe` | The application alone |
| `TreeArchiver-<version>-x64.msi` | MSI package |
| `checksums.txt` | SHA-256 of the three above |

### Testing the pipeline without releasing

Run it manually from **Actions → Release → Run workflow**. On a manual run the
version comes from `tauri.conf.json`, the version guard and the publish step are
skipped, and the binaries are attached to the workflow run as an artifact
(14-day retention) instead of a release. Use this to verify a build before
committing to a tag.

## Cutting a release

1. Bump the version in all three manifests. They must agree:

   ```bash
   npm version 1.1.0 --no-git-tag-version        # package.json + lockfile
   # then edit src-tauri/Cargo.toml  ->  version = "1.1.0"
   # and   src-tauri/tauri.conf.json ->  "version": "1.1.0"
   cd src-tauri && cargo update --workspace --offline   # refresh Cargo.lock
   ```

2. Add a `## [1.1.0] - YYYY-MM-DD` section to `CHANGELOG.md` and move the
   `[Unreleased]` entries into it. This becomes the release body verbatim, so
   write it for someone deciding whether to upgrade.

3. Commit, tag, push:

   ```bash
   git commit -am "chore(release): v1.1.0"
   git tag v1.1.0
   git push --follow-tags origin master
   ```

   `--follow-tags` pushes commits and the tag together. Pushing the tag alone
   would upload a tag pointing at a commit no branch references.

4. Watch the run under **Actions**. It takes roughly 8–15 minutes cold, less
   with a warm Rust cache.

To preview the release body before tagging:

```bash
bash .github/scripts/release-notes.sh 1.1.0
```

### If a tag was pushed with the wrong version

The guard fails the run before the build, so nothing is published. Fix the
manifests, then move the tag:

```bash
git tag -d v1.1.0
git push --delete origin v1.1.0
# commit the corrected manifests, then re-tag and push
```

Only do this for a tag whose release never published. Moving a tag people have
already fetched rewrites history out from under them.

## Repository setup

**No secrets to configure.** The workflow authenticates with the automatic
`GITHUB_TOKEN` and declares what it needs:

```yaml
permissions:
  contents: write
```

That is the only permission required — enough to create a release and upload
assets to it.

If the publish step fails with **403**, the repository is restricting the token.
Go to **Settings → Actions → General → Workflow permissions** and select *Read
and write permissions*. Under an organisation policy the equivalent setting may
be locked at the org level.

For a repository with tag protection rules, the account pushing the tag needs
permission to create it; the workflow itself never creates or moves tags.

### Third-party actions

| Action | Purpose |
| --- | --- |
| `actions/checkout@v4` | checkout |
| `actions/setup-node@v4` | Node with npm caching |
| `dtolnay/rust-toolchain@stable` | Rust toolchain |
| `Swatinem/rust-cache@v2` | caches `~/.cargo` and `src-tauri/target` |
| `actions/upload-artifact@v4` | build artifacts |
| `softprops/action-gh-release@v2` | creates the release, uploads assets |

These are pinned to major-version tags, which pick up updates automatically. To
pin against a compromised upstream, replace each with a full commit SHA:

```yaml
- uses: softprops/action-gh-release@<40-char-sha>  # v2.0.8
```

Dependabot can keep SHAs current if you add `.github/dependabot.yml` with the
`github-actions` ecosystem.

## Code signing

The builds are **unsigned**. Windows SmartScreen shows a warning on first run,
which users clear with *More info → Run anyway*. The release notes say so.

Signing needs an Authenticode certificate — an OV certificate now requires the
private key on a hardware token or in a cloud HSM, which rules out storing a
`.pfx` in repository secrets. The usual route is Azure Trusted Signing or a
provider with a signing API, invoked as a post-build step. Tauri exposes
`bundle.windows.signCommand` in `tauri.conf.json` for this. EV certificates
build SmartScreen reputation immediately; OV certificates accumulate it over
time.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `link.exe not found` | Visual Studio Build Tools missing the C++ workload |
| Version guard fails | A manifest disagrees with the tag; the annotation names which |
| `npm ci` fails on lockfile | `package-lock.json` out of sync — run `npm install` and commit it |
| Publish step returns 403 | Workflow permissions restricted; see *Repository setup* |
| Release body is a placeholder | No `## [<version>]` section in `CHANGELOG.md` |
| Blank window on launch | WebView2 runtime absent — rare, but possible on stripped Windows images |
| `fail_on_unmatched_files` error | The build produced no bundle; check the build step log |

For build logs, open the failed run under **Actions** and expand the step. The
collect step prints the file listing and checksums, which is usually the fastest
way to confirm what the bundler actually produced.

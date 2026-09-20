# Release workflow

## Trigger

Tupi releases are created automatically from pushes to `main` by
`.github/workflows/release.yml`.

The workflow uses **semantic-release** with Conventional Commits:

- `feat` → minor release
- `fix` and `perf` → patch release
- other commit types do not create a release by default

When semantic-release determines that `main` contains releasable changes, it:

1. calculates the next version;
2. updates `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`;
3. updates `CHANGELOG.md`;
4. creates a release commit and Git tag;
5. publishes a GitHub Release; and
6. triggers Windows and macOS Tauri bundle builds for that tag.

## Platform builds

The release workflow currently publishes desktop bundles for:

- Windows
- macOS

Linux is intentionally not part of the first release workflow.

## Repository requirements

The workflow relies on the repository-provided `GITHUB_TOKEN` with
`contents: write` permission so semantic-release can create tags, commits, and
GitHub Releases.

## Signing and notarization

The current workflow does **not** configure code signing or macOS notarization.
Unsigned bundles can still be produced, but platform trust prompts may be
stricter until signing credentials are added in repository secrets and wired
into the workflow.

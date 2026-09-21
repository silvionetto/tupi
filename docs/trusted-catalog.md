# Tupi Trusted Catalog

## Purpose

Tupi is responsible for deciding which AI assets are trusted. The Tupi
repository's `main` branch contains the authoritative YAML catalog of trusted
marketplaces and assets. Applications that consume Tupi assets do not define,
override, or infer trust themselves.

The desktop application refreshes this catalog from `main` and uses it to
discover and update trusted marketplaces, agents, prompts, skills, and
instructions.

An asset can be trusted only when it is listed in the catalog from the
repository's `main` branch. Assets from every other branch are untrusted,
including feature, development, release, pull-request, and local branches.

## Trusted catalog

The catalog should be stored at a stable path such as
`catalog/trusted-assets.yaml`:

```yaml
version: 1
catalogRevision: commit-sha

marketplaces:
  - id: official
    name: Tupi Official Marketplace
    repository: https://example.com/tupi-marketplace.git
    branch: main

agents:
  - id: code-reviewer
    marketplace: official
    path: agents/code-reviewer
    version: 1.2.0
    revision: commit-sha

prompts:
  - id: summarize-code
    marketplace: official
    path: prompts/summarize-code
    version: 1.0.0
    revision: commit-sha

skills:
  - id: typescript
    marketplace: official
    path: skills/typescript
    version: 1.0.0
    revision: commit-sha

instructions:
  - id: secure-coding
    marketplace: official
    path: instructions/secure-coding
    version: 1.0.0
    revision: commit-sha
```

The catalog is an allowlist, not merely an index. An asset that is absent from
the catalog must not be treated as trusted. Marketplace and asset revisions
should be pinned to immutable commits or versions rather than an unbounded
`latest` reference.

## Trust rules

1. Tupi is the authority for trust decisions.
2. Only the Tupi repository's `main` branch can publish trusted catalog
   changes.
3. A marketplace must be listed in the catalog to be trusted.
4. Catalog-listed assets must resolve to their declared marketplace and
   revision to be trusted.
5. Marketplace-owned `*.agent.md` files discovered under a trusted
   marketplace's `agents/` folder are treated as trusted marketplace agents
   and may be persisted for browsing in the desktop app.
6. A branch name must be verified by Tupi before an asset is trusted.
7. An application or profile must not implement a second trust policy.
8. An untrusted asset must never be silently promoted to trusted status by an
   application, profile, plugin, or configuration file.

## Desktop refresh workflow

The desktop application should:

1. Fetch the Tupi repository's `main` branch.
2. Verify that the fetched ref is actually `main`.
3. Resolve the catalog to a concrete repository commit.
4. Parse and validate `trusted-assets.yaml` against its schema.
5. Compare the catalog revision with the local catalog cache.
6. Download or update only the marketplaces and assets listed in the catalog.
7. Scan each trusted marketplace `agents/` folder for `*.agent.md` files and
   persist that marketplace-owned inventory with trust metadata.
8. Activate only the assets selected by the current project profile.

If the catalog cannot be fetched or verified, the application should retain the
last known valid catalog and clearly report that refresh failed. It must not
silently trust newly discovered content.

The local cache should retain the catalog revision, asset revisions, refresh
time, and validation status so the application can explain exactly which
trusted definitions are active.

## Runtime responsibilities

Tupi should:

- fetch the trusted catalog from `main`;
- verify the branch and resolve the catalog to a concrete commit;
- resolve each marketplace and asset to its declared revision;
- validate the asset manifest and dependencies;
- attach trust metadata to each resolved asset;
- reject operations that require trusted assets when the source is not `main`;
- expose the result to the desktop application through a stable API or local
  cache.

The consuming application should only use the result supplied by Tupi. It may
enforce capability or permission requirements, but it should not decide whether
the source branch is trusted.

## Trusted marketplace agent inventory

The About page may display marketplace-owned agent inventories for trusted
marketplaces. Tupi derives that inventory from the cached trusted marketplace
workspace under `.tupi/catalog-cache/marketplaces/<marketplace-id>/agents`.

When Tupi discovers a `*.agent.md` file in that trusted marketplace workspace,
it persists the agent under the owning marketplace in SQLite with the
filename-derived name, optional frontmatter `description`, and a trusted
status. This inventory is for browsing trusted marketplace content; it is
separate from the local global-agent scan under `.copilot/agents`.

## Installed Copilot marketplaces on the Home page

The desktop app may inspect the current user's Copilot installed-plugin folder
under `.copilot/installed-plugins`. Each direct child directory under that root
is treated as one installed marketplace record for Home-page inventory.

Those local directories are **not** trusted merely because they exist on the
machine. When Tupi discovers an installed marketplace directory, it persists the
marketplace metadata in SQLite and evaluates trust in the Rust core. A local
installed marketplace is trusted only when its directory name matches a trusted
catalog marketplace ID.

If Tupi cannot make that catalog match, the installed marketplace must remain
untrusted. The UI may display the result, but it must not upgrade or infer
trust on its own.

Nested plugin directories inside each installed marketplace are out of scope for
this first inventory pass and should not change the marketplace trust result by
themselves.

## Global Copilot agents on the Home page

The desktop app may also inspect the current user's global Copilot agent folder
under `.copilot/agents`. Those files are **not** trusted merely because they
exist on the local machine.

When Tupi discovers a local `*.agent.md` file, it persists the file metadata in
SQLite and evaluates trust in the Rust core. A local global agent is trusted
only when:

1. the trusted catalog lists the agent;
2. the corresponding trusted marketplace workspace already exists in
   `.tupi/catalog-cache/marketplaces`; and
3. the local file bytes exactly match the trusted agent file bytes.

If Tupi cannot make that exact match, the local global agent must remain
untrusted. The UI may display the result, but it must not upgrade or infer
trust on its own.

## Profiles

Profiles should select assets and configuration, but should not grant trust.
For example, a profile may select an asset listed in the catalog, but it cannot
add an unlisted asset or change an untrusted asset to trusted.

This keeps project configuration separate from platform governance:

- **Trusted catalog:** what Tupi allows.
- **Profile:** what the project wants to load.
- **Desktop application:** refreshes the catalog and enforces both decisions.
- **Asset:** the content ultimately used by the project.

## Implementation direction

The first implementation should centralize catalog and trust enforcement in
the Tupi desktop application or its shared runtime:

```text
tupi refresh
  -> fetch Tupi main
  -> verify branch and catalog commit
  -> parse and validate trusted-assets.yaml
  -> resolve pinned marketplaces and assets
  -> update local catalog and asset cache
```

The trust check should happen before an asset is installed into the trusted
runtime environment. If branch, catalog, manifest, or revision verification
fails, Tupi should fail closed rather than treating the asset as trusted.

Later versions can add signed commits, publisher identity, checksums, and
approval workflows. These strengthen provenance, but they do not replace the
core rule: Tupi trusts the catalog published from `main`; all other branches
are untrusted.

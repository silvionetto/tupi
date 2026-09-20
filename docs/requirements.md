# Tupi Requirements

## 1. Product overview

Tupi is a desktop application for managing trusted AI assets across projects.
It provides a central catalog of marketplaces and assets, refreshes that
catalog from the Tupi repository, and lets users activate project-specific
profiles.

Supported asset categories are:

- marketplaces;
- agents;
- prompts;
- skills; and
- instructions.

Tupi is the trust authority. Consuming applications may use the assets
resolved by Tupi, but they must not define a separate trust policy.

## 2. Goals

Tupi must:

1. provide a single, versioned source of truth for trusted AI assets;
2. allow the desktop application to refresh the latest trusted catalog;
3. ensure only catalog-approved content is treated as trusted;
4. support multiple project profiles with different enabled assets;
5. preserve reproducibility by pinning catalog, marketplace, and asset
   revisions; and
6. fail closed when new content cannot be verified.

## 3. Scope

### In scope

- the cross-platform desktop application;
- the trusted YAML catalog stored in the Tupi repository;
- catalog parsing, schema validation, and branch verification;
- marketplace and asset revision resolution;
- local catalog and asset caching;
- project profile creation, selection, and activation;
- trust status and refresh diagnostics;
- a Rust/Tauri implementation with a TypeScript user interface and SQLite
  local state.

### Out of scope for the MVP

- running agents or tools as a hosted service;
- creating or publishing marketplace content from the desktop application;
- an online marketplace payment or rating system;
- automatic trust for arbitrary third-party repositories;
- trust decisions implemented by consuming applications;
- signed publisher identities, approval workflows, and enterprise policy
  administration.

These may be added later, but they must not weaken the branch and catalog
rules defined here.

## 4. Trust model

### 4.1 Authority

The Tupi repository's `main` branch is the only source permitted to publish
trusted catalog changes. Tupi, through its native core, evaluates and enforces
the catalog. The UI, profiles, and consuming applications cannot override that
decision.

### 4.2 Trust conditions

An item is trusted only when all of the following are true:

1. the Tupi catalog was fetched from and verified against `main`;
2. the catalog is valid according to the catalog schema;
3. the item is explicitly listed in the catalog;
4. its marketplace is listed and resolves to the declared repository;
5. its branch is verified as exactly `main`;
6. its revision matches the catalog entry; and
7. its manifest and dependencies pass validation.

Anything from another branch, an unlisted source, an unpinned revision, or an
unverifiable source is untrusted.

### 4.3 Fail-closed behavior

If Tupi cannot verify the branch, catalog revision, manifest, dependency, or
asset revision, it must not mark the content as trusted. It must report the
failure explicitly.

During a refresh failure, Tupi may continue using the last known valid catalog
and cache, but it must identify that state as stale and must not add newly
discovered content.

## 5. Trusted catalog

The authoritative catalog should be stored at:

```text
catalog/trusted-assets.yaml
```

The initial schema should support:

```yaml
version: 1
catalogRevision: commit-sha

marketplaces:
  - id: official
    name: Tupi Official Marketplace
    repository: https://example.com/tupi-marketplace.git
    branch: main
    revision: commit-sha

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

The catalog is an allowlist, not just an index. Entries should use immutable
commit identifiers or immutable versions. An unbounded `latest` reference must
not be accepted for trusted content.

The schema must reject:

- duplicate IDs within an asset category;
- references to unknown marketplaces;
- missing revisions;
- unsupported asset categories;
- invalid repository URLs;
- empty paths or IDs; and
- malformed YAML.

## 6. Desktop application requirements

### 6.1 Catalog refresh

The application must provide a refresh operation that:

1. fetches the Tupi repository's `main` branch;
2. verifies that the fetched ref is actually `main`;
3. resolves the catalog to a concrete commit;
4. parses and validates `catalog/trusted-assets.yaml`;
5. compares the catalog revision with local state;
6. resolves only catalog-listed marketplaces and assets;
7. validates manifests, revisions, and dependencies;
8. atomically updates the local catalog and asset cache; and
9. reports the resulting catalog revision and refresh status.

The application should support both manual refresh and a configurable
background refresh. Background refresh must not interrupt an active project
session or replace a valid cache with a partially downloaded state.

### 6.2 Local cache

The cache must retain, at minimum:

- the last valid catalog contents;
- the catalog commit/revision;
- each resolved marketplace and asset revision;
- trust status;
- last successful refresh time;
- last attempted refresh time;
- validation errors; and
- stale/offline status.

Cache updates must be atomic so that an interrupted refresh cannot leave a
profile referencing incomplete content.

### 6.3 Asset management

Users must be able to:

- browse catalog-approved assets;
- view source, branch, revision, version, and trust status;
- refresh or retry failed downloads;
- remove cached assets; and
- see why an asset is unavailable or untrusted.

The application must not present an untrusted asset as trusted through labels,
icons, default filters, or generated profile data.

## 7. Project profiles

A profile describes which catalog-approved assets a project wants to use. It
may contain:

- a profile ID and display name;
- a saved project location that points to the repository root;
- an optional description for the project;
- selected marketplace and asset IDs;
- project-specific configuration values;
- enabled/disabled state;
- an optional profile version; and
- the catalog revision against which the profile was resolved.

Profiles must not contain a field that grants trust. A profile cannot add an
unlisted asset, change a source branch, replace a revision, or convert an
untrusted item into a trusted item.

When a selected asset is removed from the catalog or becomes unverifiable,
Tupi must mark the profile resolution as incomplete and identify the affected
asset. It must not silently substitute another asset.

## 8. Proposed architecture

### 8.1 Technology

- **Desktop shell:** Tauri.
- **Trust and catalog core:** Rust.
- **User interface:** TypeScript with the selected Tauri-compatible UI
  framework.
- **Catalog parsing and validation:** Rust types using `serde` and YAML
  schema validation.
- **Local persistence:** SQLite.
- **Repository and revision operations:** a controlled Git integration owned by
  the Rust core.

### 8.2 Responsibility boundaries

The Rust core owns:

- repository fetching and branch verification;
- catalog parsing and validation;
- trust decisions;
- revision and dependency resolution;
- cache writes;
- profile resolution; and
- errors that affect trust or integrity.

The TypeScript UI owns presentation and user interaction. It requests
operations from the Rust core through typed Tauri commands and displays the
results. It must not independently fetch, parse, or authorize trusted assets.

## 9. Functional requirements

| ID | Requirement |
|---|---|
| FR-001 | The application shall load the trusted catalog only from the configured Tupi repository `main` branch. |
| FR-002 | The application shall verify the branch and resolve a concrete catalog commit before accepting catalog changes. |
| FR-003 | The application shall reject malformed or schema-invalid catalogs. |
| FR-004 | The application shall treat the catalog as an allowlist. |
| FR-005 | The application shall require immutable marketplace and asset revisions. |
| FR-006 | The application shall validate that every asset references a known marketplace. |
| FR-007 | The application shall expose trust status and provenance for every resolved asset. |
| FR-008 | The application shall preserve the last valid cache when refresh fails. |
| FR-009 | The application shall prevent partial cache updates. |
| FR-010 | The application shall support creating, selecting, editing, and deleting project profiles. |
| FR-011 | The application shall resolve profiles only against the trusted catalog. |
| FR-012 | The application shall identify unavailable, stale, untrusted, and invalid assets explicitly. |
| FR-013 | The application shall prevent the UI or profile data from overriding core trust decisions. |
| FR-014 | The application shall support offline use of the last valid catalog with visible stale status. |

## 10. Non-functional requirements

| ID | Requirement |
|---|---|
| NFR-001 | Trust enforcement shall be implemented in the Rust core, not only in the UI. |
| NFR-002 | Catalog and asset updates shall be atomic and recoverable after interruption. |
| NFR-003 | Errors affecting trust, provenance, or integrity shall be visible to the user and available in diagnostic logs. |
| NFR-004 | The application shall not silently downgrade verification failures into trusted status. |
| NFR-005 | Core catalog, trust, and profile logic shall be testable without the graphical UI. |
| NFR-006 | The application shall support the target desktop operating systems selected during project setup. |
| NFR-007 | Local secrets and credentials, if later required, shall not be stored in the YAML catalog or profiles. |
| NFR-008 | Catalog and profile formats shall be versioned for forward-compatible migrations. |

## 11. MVP acceptance criteria

The MVP is acceptable when:

1. a valid catalog committed to `main` can be refreshed and cached;
2. a catalog from a non-`main` branch is rejected as trusted;
3. malformed YAML and schema-invalid entries are rejected with actionable
   errors;
4. an unlisted marketplace or asset cannot be activated as trusted;
5. pinned revisions are displayed and used for resolution;
6. a failed refresh preserves the previous valid catalog and marks it stale;
7. a user can create two profiles with different approved asset selections;
8. changing a profile cannot change an asset's trust status;
9. an interrupted refresh cannot produce a partially valid cache; and
10. the UI displays the source branch, commit/revision, trust status, and
    refresh state for selected assets.

## 12. Future considerations

The architecture should leave room for:

- signed catalog commits;
- publisher identity and signature verification;
- organization-managed catalog mirrors;
- approval workflows for catalog changes;
- dependency lockfiles;
- asset compatibility constraints;
- import/export of profiles; and
- a graphical marketplace browser.

These features are extensions to the trust model. They must not replace the
fundamental rule that trusted catalog content is published from `main` and
enforced by Tupi.

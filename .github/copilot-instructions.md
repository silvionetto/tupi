# Tupi repository instructions

## Build and test commands

| Command | Use |
|---|---|
| `npm run dev` | Start only the Vite frontend shell. Tauri commands will not be available in this mode. |
| `npm run desktop:dev` | Start the Tauri desktop app with the Rust backend; use this when working on catalog refresh, profile persistence, or marketplace agent discovery. |
| `npm run build` | Build the React frontend into `dist/` for the Tauri bundle step. |
| `cargo build --manifest-path src-tauri/Cargo.toml` | Build the Rust/Tauri backend crate. |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Run the Rust unit tests. |
| `cargo test --manifest-path src-tauri/Cargo.toml rejects_duplicate_marketplaces` | Run a single Rust test by name. |

There is no dedicated lint script or formatter command checked in today.

## High-level architecture

Tupi is a Tauri desktop app with a strict split between a thin React UI in `src/` and a Rust trust core in `src-tauri/src/`. The UI should stay presentation-focused: `src/App.tsx` loads catalog state and profiles by calling Tauri commands with `invoke`, then renders catalog status, refresh controls, and profile CRUD. Trust decisions, catalog parsing, repository verification, and persistence all live on the Rust side.

The Rust backend centers on `AppState` in `src-tauri/src/state.rs`. It owns local state under `.tupi/`, including the SQLite database at `.tupi/state.sqlite` and the cloned trusted source cache under `.tupi/catalog-cache/`. `main.rs` registers Tauri commands from `commands.rs`, which are intentionally thin wrappers over `AppState`. `catalog.rs` defines the YAML schema and validation rules for `catalog/trusted-assets.yaml`, while `state.rs` handles refreshes by fetching the catalog source, enforcing `main`, validating the catalog, and atomically upserting the cached summary plus raw YAML into the single-row `catalog_cache` table. The same Rust layer also resolves profile agent options by scanning cached trusted marketplace workspaces under `.tupi/catalog-cache/marketplaces`. Profiles are stored separately as JSON blobs in the `profiles` table and round-tripped through `profile.rs`.

The trust model in `README.md` and `docs/trusted-catalog.md` is not just documentation; the code is built around it. `catalog/trusted-assets.yaml` is the single source of truth for trusted marketplaces and assets, and the bundled file is the local fallback sample. On startup, `load_catalog_state()` prefers the cached catalog; otherwise it parses the bundled YAML and reports it as stale scaffold/local state until a successful refresh replaces it.

## Key repository conventions

- **Rust owns trust; the UI must not re-implement it.** Keep branch checks, catalog validation, trust status, and provenance in Rust/Tauri commands rather than in React.
- **The catalog is an allowlist, not discovery data.** `catalog/trusted-assets.yaml` must explicitly list every trusted marketplace/asset, and validation rejects duplicate IDs, empty identifiers, unknown marketplaces, invalid URLs, non-`main` branches, and mutable revisions such as `latest`.
- **Use one trust source.** Keep marketplace and asset trust in `catalog/trusted-assets.yaml`; do not reintroduce a second allowlist file for repository trust.
- **Cache writes are designed to preserve the last valid state.** Refresh logic writes catalog updates inside a SQLite transaction and stores both the serialized summary and the raw YAML in `catalog_cache`; code changes in this path should preserve fail-closed behavior and stale-cache fallback.
- **Profiles select assets but never grant trust.** Profile records contain user choices (`selected_assets`, display metadata, enabled flag, optional `catalogRevision`), but they do not override catalog trust status.
- **Profile agent options come from trusted marketplace workspaces.** The picker should list Markdown files under each trusted marketplace repo's `agents/` folder, expose them through a filterable listbox, and store only the filename stem in `selected_assets`.
- **Serde field shapes matter across the Tauri boundary.** The frontend expects the Rust payload shape as serialized today, including `catalogRevision` for catalog/profile revisions and snake_case fields like `trust_status`, `source_branch`, and `selected_assets`.
- **Schema changes require migration code, not just SQL edits.** `AppState::initialize()` plus `ensure_cache_columns()` are the repository’s existing pattern for evolving the SQLite schema without breaking older local state.
- **Persist durable knowledge in `docs/` before ending a session.** When a session uncovers reusable repository knowledge, capture it as focused Markdown under `docs/` instead of leaving it only in chat context. Prefer small topic-based files such as `docs/profile-agent-picker.md`, with concise headings, decision summaries, invariants, command examples, and links to the canonical code/docs so future agents can scan only the relevant file and spend fewer tokens.

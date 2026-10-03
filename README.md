# TUPI

## IDEA

Tupi is a centralized repo for AI tools.
Tupi contains AI Marketplaces with its Plugins, Agents, Tools, Instructions, Prompts and Skills.
With Tupi you can switch between profiles for each project.
Each Profile will have a set of agents and skills for a specific project.
Using Tupi you will be able to choose which agents and skills do you want to load in your project from trusted sources.

## MVP scaffold

This repository now contains the first desktop-app scaffold for the MVP:

- `catalog/trusted-assets.yaml` is the authoritative trusted catalog sample.
- `catalog_repository.yml` lists trusted repository sources used during refresh.
- `src-tauri/` contains the Rust trust/catalog core, SQLite state, and Tauri commands.
- `src/` contains the React frontend shell.

The current implementation focuses on the trust-model foundation: catalog loading and validation, local cache persistence, profile storage, and a React frontend wired to the Tauri commands. The remaining core step is hardening the refresh path around repository verification in a real Tupi catalog source.

Project profiles are stored locally in the existing embedded SQLite database at `.tupi/state.sqlite`. The current Profiles page focuses on project metadata: a database-backed ID, the saved repository root location, a display name guessed from that folder name and still editable, and an optional description.

The Home page now scans the current user's Copilot installation folders on application startup. Tupi inventories installed marketplaces from `%USERPROFILE%\.copilot\installed-plugins` by treating each direct child directory as one installed marketplace record, persists that inventory in SQLite, and marks a marketplace as trusted only when its directory name matches a catalog-listed trusted marketplace. Direct child plugin folders are listed under each marketplace in collapsed sections; expanding a plugin shows its skill folders under `skills/` that contain a `SKILL.md` file, alongside agent files directly inside its `agents/` directory that end in `*.agent.md`. Both asset lists show their names and locations, and agent files also show an optional frontmatter `description`. Tupi also scans `%USERPROFILE%\.copilot\agents`, stores each discovered `*.agent.md` file with its filename-derived agent name, file location, optional frontmatter `description`, and a trust flag, and marks a global agent as trusted only when its file bytes exactly match a catalog-listed trusted agent file already present in Tupi's trusted marketplace cache.

Tupi syncs each catalog-listed marketplace at its pinned revision into `.tupi/catalog-cache/marketplaces`. The About page browses marketplace-owned agents and skills from those cached trusted workspaces. In Profiles, **Agents & skills** opens a project-specific manager for copying an agent to `.github/agents` or a skill folder to `.github/skills`. The Rust backend verifies the active catalog and pinned marketplace source. Installed asset provenance and content digests are stored in Tupi's local SQLite state. Existing destination conflicts are not overwritten; uninstall removes only recorded copies whose contents still match the installation. Modified copies are preserved and reported.

### Local refresh configuration

- The trusted catalog defaults to `https://github.com/silvionetto/tupi.git` on `main`.
- `TUPI_CATALOG_REPOSITORY`: optional override for deployments using a trusted catalog mirror.
- `TUPI_CATALOG_BRANCH`: branch override; the MVP only accepts `main`.

On first launch, the bundled `catalog/trusted-assets.yaml` sample is marked stale scaffold data.
Open a profile's **Agents & skills** manager and choose **Refresh trusted catalog** before installing
assets. Asset installation remains disabled until Tupi successfully reads the catalog from the
configured repository's `main` branch.

## Development

Use one of these commands from the repository root to run the app during development:

- `npm run dev` — starts the Vite frontend shell only. This is useful for UI work that does not require the Tauri desktop backend.
- `npm run desktop:dev` — starts the desktop app with the Tauri Rust backend attached. This is the recommended option when working on catalog refresh, profile persistence, or marketplace discovery.

If you need a full backend build check as well, run:

- `cargo build --manifest-path src-tauri/Cargo.toml`

## Contributor conventions

- Use **Conventional Commits** for PR titles and future commit messages: `<type>(<scope>): <summary>`
- Prefer scopes such as `core`, `ui`, `catalog`, `profiles`, `release`, and `docs`
- See `docs/conventional-commits.md` for the agreed format, examples, and release mapping
- See `docs/release-workflow.md` for the GitHub Actions + semantic-release publishing flow

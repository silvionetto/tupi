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

The Home page now scans the current user's Copilot installation folders on application startup. Tupi inventories installed marketplaces from `%USERPROFILE%\.copilot\installed-plugins` by treating each direct child directory as one installed marketplace record, persists that inventory in SQLite, and marks a marketplace as trusted only when its directory name matches a catalog-listed trusted marketplace. Tupi also scans `%USERPROFILE%\.copilot\agents`, stores each discovered `*.agent.md` file with its filename-derived agent name, file location, optional frontmatter `description`, and a trust flag, and marks a global agent as trusted only when its file bytes exactly match a catalog-listed trusted agent file already present in Tupi's trusted marketplace cache.

Tupi also syncs trusted marketplace workspaces into `.tupi/catalog-cache/marketplaces` and now persists marketplace-owned `*.agent.md` files found under each trusted marketplace `agents/` folder. Those marketplace agents are stored in SQLite under their marketplace and shown on the About page inside each marketplace's `agents` section with the filename-derived name and optional frontmatter description.

### Local refresh configuration

- `TUPI_CATALOG_REPOSITORY`: optional git repository URL used for trusted catalog refreshes
- `TUPI_CATALOG_BRANCH`: trusted branch name; the MVP only accepts `main`

If no repository is configured, the app falls back to the bundled `catalog/trusted-assets.yaml` sample and marks the state as local scaffold data.

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

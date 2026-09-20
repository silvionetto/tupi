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

The Home page now scans the current user's global Copilot agent folder at `%USERPROFILE%\.copilot\agents` on application startup. Tupi stores each discovered `*.agent.md` file in the local SQLite database with its filename-derived agent name, file location, optional frontmatter `description`, and a trust flag. A global agent is marked trusted only when its file bytes exactly match a catalog-listed trusted agent file already present in Tupi's trusted marketplace cache.

### Local refresh configuration

- `TUPI_CATALOG_REPOSITORY`: optional git repository URL used for trusted catalog refreshes
- `TUPI_CATALOG_BRANCH`: trusted branch name; the MVP only accepts `main`

If no repository is configured, the app falls back to the bundled `catalog/trusted-assets.yaml` sample and marks the state as local scaffold data.

## Contributor conventions

- Use **Conventional Commits** for PR titles and future commit messages: `<type>(<scope>): <summary>`
- Prefer scopes such as `core`, `ui`, `catalog`, `profiles`, `release`, and `docs`
- See `docs/conventional-commits.md` for the agreed format, examples, and release mapping

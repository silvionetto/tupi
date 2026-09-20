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

### Local refresh configuration

- `TUPI_CATALOG_REPOSITORY`: optional git repository URL used for trusted catalog refreshes
- `TUPI_CATALOG_BRANCH`: trusted branch name; the MVP only accepts `main`

If no repository is configured, the app falls back to the bundled `catalog/trusted-assets.yaml` sample and marks the state as local scaffold data.

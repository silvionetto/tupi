# Profile agent selection

The **Profiles** view is a single master-detail page. The master list selects a
profile and provides profile deletion; the detail pane creates or edits profile
metadata and the profile's selected trusted agents.

Agent choices come from the Rust `list_marketplaces` command, which resolves
marketplaces and agents from the active trusted catalog. The React UI does not
discover agents or decide trust. Adding or removing an agent updates the
profile's `selected_assets` list; save the profile to persist those changes.
The list stores each agent's filename stem, matching the existing profile
payload contract.

Agent selection is distinct from project asset installation. **Manage
installed assets** opens the existing install/uninstall flow for files in the
project. Selecting an agent does not copy or remove any files.

Canonical implementation: `src/App.tsx`; profile persistence and
normalization: `src-tauri/src/state/profiles_store.rs`.

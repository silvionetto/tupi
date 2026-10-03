# Global agent and skill inventory

Home inventories per-user agents and skills from the user's `.copilot` data:

- direct global agents under `.copilot/agents`;
- direct global skills under `.copilot/skills`; and
- agents and skills inside `.copilot/installed-plugins`.

The React view presents these as **Global → Agents / Skills**. It displays
each item's location, trust status, and (for plugin assets) marketplace/plugin
context. Scan and removal failures are shown in the UI.

## Trust

Rust owns trust classification. Direct global agents must exactly match the
content of an agent listed in the active trusted catalog. Direct global skills
are compared by content digest with catalog-listed skill content. Agents and
skills found inside an installed marketplace plugin inherit that marketplace's
trust status, based on whether its marketplace ID is listed in the active
catalog. Unmatched marketplaces and assets remain untrusted.

## Removal behavior

Trusted items use **Uninstall**; untrusted items use **Delete**. Both remove
only the selected agent file or skill directory and preserve parent plugin and
marketplace directories. The Rust command re-scans the relevant inventory,
requires the selected path to still be discovered, and checks that the target
is an agent or skill inside its expected user-level root. It rejects invalid
path components, symlink traversal, and paths outside that root. The UI asks
for confirmation and refreshes all three inventories after the operation.

This inventory is independent of project profiles, which continue to manage
project-local installations through their existing profile asset commands.

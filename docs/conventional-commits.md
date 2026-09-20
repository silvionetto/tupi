# Conventional Commits for Tupi

## Decision

Tupi will use **Conventional Commits** for pull request titles and future commit messages:

```text
<type>(<scope>): <summary>
```

Examples:

- `feat(core): add catalog refresh diagnostics`
- `fix(ui): preserve selected assets when editing a profile`
- `docs(readme): describe trusted catalog refresh`
- `ci(release): publish Tauri bundles from semantic-release`

## Types

Use these types by default:

| Type | Meaning | Release impact |
|---|---|---|
| `feat` | New user-visible or developer-visible capability | Minor |
| `fix` | Bug fix | Patch |
| `perf` | Performance improvement | Patch |
| `refactor` | Internal restructuring without behavior change | None by default |
| `docs` | Documentation-only change | None by default |
| `test` | Test-only change | None by default |
| `build` | Build tooling or dependencies | None by default |
| `ci` | CI/CD workflow changes | None by default |
| `chore` | Repository maintenance | None by default |
| `style` | Formatting-only change | None by default |

## Scopes

Scopes should describe the area changed. Common examples for this repository:

- `core`
- `ui`
- `catalog`
- `profiles`
- `release`
- `docs`
- `tauri`
- `rust`

`core` is a **scope**, not a type. Prefer:

```text
feat(core): ...
fix(core): ...
```

Avoid:

```text
core: ...
```

## Breaking changes

Mark breaking changes in either of these ways:

```text
feat(core)!: replace profile schema with catalog-pinned assets
```

or

```text
feat(core): replace profile schema with catalog-pinned assets

BREAKING CHANGE: profiles must be migrated to the new selected_assets format.
```

## Copilot guidance vs enforcement

GitHub Copilot can be guided with repository instructions such as:

- `.github/copilot-instructions.md`
- `.github/instructions/**/*.instructions.md`

That guidance helps Copilot produce better commit messages and PR titles, but it does **not** guarantee enforcement.

For real enforcement, use repository automation such as:

1. PR-title validation in GitHub Actions
2. commitlint for local or CI validation
3. branch protection that requires the validation check to pass

For the current rollout, the planned first step is **PR-title enforcement** because it is lower friction than enforcing every commit immediately.

# AGENTS.md

Instructions for AI coding agents (Cursor, Codex, Claude Code, etc.) when editing **agent-configs**.

## Repository purpose

This repo holds **reusable agent configuration**: Cursor/Claude plugin layouts, **agentpack**-ready packages, optional standalone **skills**, and **commands**. Changes should stay consistent with how each subtree is consumed (editor discovery, agentpack staging, or copy-paste).

## Top-level layout

| Path | Role |
| --- | --- |
| **`plugins/`** | Shippable plugin trees (manifests, `agentpack.toml`, bundled skills/agents). This is the default home for ecosystem bundles. |
| **`drafts/build-go-stack/`** | Single source of truth for Go conventions. Extend this skill when Go guidance changes. |
| **`skills/`** | Standalone skills meant to be copied or wired into a project/plugin without pulling a whole bundle. |
| **`commands/`** | Shared command markdown (frontmatter + body). Mirror the style of `commands/reflect.md`. |
| **`workspaces/`** | Skill eval / benchmark workspaces (inputs, runs, grading). Not packaged for end users; keep churn localized here. |
| **`agentpack.toml`** (+ **`pack.lock`**) | Repo-level agentpack metadata / dev dependencies (e.g. upstream skills for authoring). |

## `plugins/` — packaged plugins

- **`plugins/sample-plugin/`** — Reference layout: rules, skills, agents, commands, hooks, MCP stub, dual Cursor/Claude manifests. Preserve it as a small exemplar unless the task is to change that reference. See its README for symlink install.
- **`plugins/rust-dev/`** — Rust ecosystem skills. New Rust skills for this bundle: `plugins/rust-dev/skills/<skill-id>/SKILL.md`.
- **`plugins/simplifier/`** — Small plugin (e.g. agents); extend only as that package grows.

## Where to add a **new skill** (choose one)

1. **Go** → `drafts/build-go-stack/` (`SKILL.md` and `references/`). That draft is the only Go stack.
2. **Rust-specific** (this repo’s Rust bundle) → `plugins/rust-dev/skills/<skill-id>/SKILL.md`
3. **Cross-cutting or language-agnostic** → `skills/<skill-id>/SKILL.md` (YAML frontmatter: `name`, `description`)
4. **Demo / template only** (for the reference plugin) → `plugins/sample-plugin/skills/<skill-id>/SKILL.md`

Do not create extra nesting like `skills/skills/`; each skill is exactly one directory with `SKILL.md` inside.

## Conventions

The canonical Nudge CLI skill lives here at `skills/nudge-cli/`, including its `references/` directory. Make future Nudge skill changes in this repository, even when prompted by CLI changes in `olegpustovit/nudge`; do not maintain a second copy in the application repository.

1. **Skills:** Use `---` frontmatter at the top of `SKILL.md`. The `description` should state when to load the skill (triggers, keywords, “use when…”).
2. **Plugins:** Cursor manifest is under `.cursor-plugin/plugin.json`; Claude under `.claude-plugin/plugin.json` where both exist. Respect documented `logo` paths and default discovery folders unless explicitly overriding paths in the manifest.
3. **Hooks/scripts:** If you add hook scripts, they must be executable where the repo expects that; prefer no-op or clearly safe behavior for templates.
4. **MCP:** Do not turn placeholders into real credentials or production servers without explicit user direction.

## Scope of edits

- Prefer **minimal diffs** that match surrounding style.
- Do not rename plugin package directories or manifest `name` fields casually — that can break users’ symlinks and agentpack keys.
- Avoid adding repo-wide docs the user did not ask for; root `README.md` and this file are the primary navigation.

## Verification

After structural changes to a plugin, sanity-check that paths referenced from `plugin.json` (e.g. `logo`, custom rule/skill dirs) still exist.

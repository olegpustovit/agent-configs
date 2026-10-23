# sample-plugin

A reference plugin directory with Cursor and Claude manifests, a sample skill and no-op hooks. Use it to inspect the package layout before building a real plugin.

## Inspect locally

From the repository root, with Node.js installed:

```sh
node docs/demo/check.cjs
```

The check parses the manifests, verifies the logo and skill paths, and executes only scripts confirmed to contain `exit 0`. It does not launch an editor or an MCP server.

## Contents

| Path | Purpose |
| --- | --- |
| `.cursor-plugin/plugin.json` | Cursor manifest with `assets/logo.svg` |
| `.claude-plugin/plugin.json` | Claude manifest |
| `agentpack.toml` | Nested package metadata with no dependencies |
| `rules/` | Example rule definitions |
| `skills/sample-skill/SKILL.md` | Example skill entry point |
| `agents/` and `.claude/agents/` | Example agent definitions |
| `commands/` and `.opencode/commands/` | Example commands |
| `hooks/hooks.json` | Hook events and script paths |
| `scripts/` | No-op hook scripts |
| `mcp.json` | MCP configuration placeholder |

The MCP placeholder uses `/bin/true`. It exits immediately and does not implement MCP. Replace it with a real server configuration before trying a connection.

## Integrate with an editor or agentpack

Use the installation mechanism documented for your editor version. The local inspection does not verify plugin discovery, hook event compatibility or agentpack staging. This directory's two manifests are examples; they are not a cross-editor compatibility guarantee.

- [Cursor plugin reference](https://cursor.com/docs/reference/plugins.md)
- [agentpack repository](https://github.com/olegpustovit/agentpack)

Preserve the directory layout and manifest names when using this as a reference. If you change manifest paths, check that the referenced files exist.

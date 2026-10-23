// Inspect the shipped reference plugin and execute only its checked-in no-op hooks.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../plugins/sample-plugin');
for (const file of ['.cursor-plugin/plugin.json', '.claude-plugin/plugin.json']) {
  const manifest = JSON.parse(fs.readFileSync(path.join(root, file), 'utf8'));
  console.log(`${file}: ${manifest.name} ${manifest.version}`);
  if (manifest.logo && !fs.existsSync(path.join(root, manifest.logo))) throw Error('Missing logo');
}
console.log('Rules: ' + fs.readdirSync(path.join(root, 'rules')).join(', '));
if (!fs.existsSync(path.join(root, 'skills/sample-skill/SKILL.md'))) throw Error('Missing sample skill');
console.log('Skill: skills/sample-skill/SKILL.md');
const mcp = JSON.parse(fs.readFileSync(path.join(root, 'mcp.json'), 'utf8'));
if (mcp.mcpServers['sample-placeholder'].command !== '/bin/true') throw Error('Unexpected MCP command');
console.log('MCP: /bin/true placeholder, not a running server');
const hooks = JSON.parse(fs.readFileSync(path.join(root, 'hooks/hooks.json'), 'utf8')).hooks;
for (const [event, entries] of Object.entries(hooks)) {
  for (const entry of entries) {
    const script = path.resolve(root, entry.command);
    if (path.dirname(script) !== path.join(root, 'scripts')) throw Error('Unexpected hook path');
    const text = fs.readFileSync(script, 'utf8');
    if (!/^#![^\n]+\n(?:#[^\n]*\n)*exit 0\s*$/.test(text)) throw Error('Hook is not a plain no-op');
    const result = spawnSync('sh', [script], {encoding:'utf8'});
    if (result.status !== 0) throw Error('Hook failed');
    console.log(`${event}: no-op hook exited ${result.status}`);
  }
}
console.log('No editor, agent session or MCP server was started.');

# Fast UI checks, build, and staging

Use this reference to keep frontend iteration independent from Go while preserving a correct combined artifact.

## Scripts

```json
{
  "scripts": {
    "dev": "vite",
    "lint": "oxlint",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "build": "vite build && tsc --noEmit && bun run size",
    "size": "bun scripts/check-size.ts",
    "preview": "vite preview"
  }
}
```

Run them with `bun run <script>`. `bun run test` runs Vitest; do not use `bun test`, which is Bun's own runner with different APIs. The router plugin regenerates `routeTree.gen.ts` during `dev` and `build`; when typechecking without a build, run the generator first (`tsr generate` from `@tanstack/router-cli`, or one `vite build`) so route types are current.

`typescript` 7 is the native Go compiler: plain `tsc --noEmit` is the fast typecheck. Do not add `@typescript/native-preview` or a second checker. If a repository pins TypeScript 5, upgrade it rather than keeping two compilers.

## Compile with React Compiler

Install `babel-plugin-react-compiler`, `@rolldown/plugin-babel`, `@babel/core@7`, and `@types/babel__core`, and register `babel({ presets: [reactCompilerPreset()] })` after `react()` (see [frontend-integration.md](frontend-integration.md)). The compiler memoizes components and hooks, so do not add `useMemo`, `useCallback`, or `memo` by hand unless a profile shows the compiler bailed out of that component. Keep `@babel/core` on 7: Workbox (used by the PWA plugin) fails to write `sw.js` under Babel 8. Run `oxlint` with its React rules so code that breaks the Rules of React fails before the compiler silently skips it.

`@vitejs/plugin-react` also offers `react({ compiler: true })` through the Rust `oxc-transform-react` port. It is faster but marked experimental; switch only after its warning is removed from the plugin README.

Configure Vitest in `vite.config.ts` (import `defineConfig` from `vitest/config`; `test: { environment: "jsdom", setupFiles: [...] }`) so tests share aliases and plugins. Render components under a test `QueryClientProvider` (retries off), and, for routed components, a memory-history router.

## Keep targets separate

```text
ui-lint       -> UI source and lint configuration
ui-typecheck  -> UI source, generated route tree, TypeScript configuration
ui-test       -> UI source and Vitest configuration
ui-install    -> package.json + bun.lock
ui-build      -> UI source/config + public/ + bun.lock
ui-stage      -> ui-build output + build verification + content fingerprint
artifact      -> fresh ui-stage + build-go-stack backend artifact + fallback smoke test
```

Do not make the normal backend test or build depend on `ui-build`. The complete `artifact` target must prove that the staged build matches current UI inputs.

## Preserve caches and content identity

- Preserve Bun's install cache and Vite's dependency cache (`node_modules/.vite`) locally and in CI.
- Install a worktree-local `node_modules`; do not symlink it outside the worktree.
- Fingerprint `src/` (including `routeTree.gen.ts`), `public/`, `index.html`, `vite.config.ts`, `tsconfig*.json`, `components.json`, `package.json`, and `bun.lock`.
- Exclude `dist`, `node_modules`, logs, and unrelated backend files from the fingerprint.
- Build into `ui/dist`, verify it, copy to a temporary staging directory, then replace the generated embed directory safely.
- Do not overwrite staged files when contents are identical; timestamp churn invalidates the Go embed package and forces relinking.
- Update the fingerprint only after build, copy, and verification succeed.

## Enforce the speed budget

Every app must stay fast on a mid-range phone. `scripts/check-size.ts` fails the build when the budget is exceeded:

| Measure (gzip) | Budget |
| --- | --- |
| JS loaded by `index.html` (entry plus its static imports) | 170 KB |
| CSS loaded by `index.html` | 30 KB |
| Any single lazy route chunk | 100 KB |

```ts
// ui/scripts/check-size.ts — run after vite build
import { readFileSync, readdirSync } from "node:fs"
import { gzipSync } from "node:zlib"

const dist = new URL("../dist/", import.meta.url)
const gz = (f: string) => gzipSync(readFileSync(new URL(f, dist))).length
const html = readFileSync(new URL("index.html", dist), "utf8")
const initial = [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.(?:js|css))"/g)].map((m) => m[1])
const sum = (ext: string) => initial.filter((f) => f.endsWith(ext)).reduce((n, f) => n + gz(f), 0)
const failures: string[] = []
if (sum(".js") > 170_000) failures.push(`initial JS ${sum(".js")} B > 170 KB`)
if (sum(".css") > 30_000) failures.push(`initial CSS ${sum(".css")} B > 30 KB`)
for (const f of readdirSync(new URL("assets/", dist)))
  if (f.endsWith(".js") && !initial.includes(`assets/${f}`) && gz(`assets/${f}`) > 100_000)
    failures.push(`chunk ${f} ${gz(`assets/${f}`)} B > 100 KB`)
if (failures.length) { console.error(failures.join("\n")); process.exit(1) }
```

- Vite emits `<link rel="modulepreload">` for the entry's static imports, so the regex counts them as initial.
- When a chunk exceeds its budget, split it before raising the number: load heavy editors, PDF viewers, charts, and syntax highlighters with `React.lazy` inside the route that needs them, and never import them from `__root.tsx` or shared layouts.
- Do not import whole icon packs, `lodash`, `moment`, or locale bundles. Use `Intl` for dates, numbers, and relative time before reaching for a date library.
- Self-host fonts as `woff2` subsets; preload only the one weight used above the fold.
- Raise a budget only with a written reason in the repository's AGENTS.md.

## Verify the build before staging

- `index.html` exists, uses absolute `/assets/...` URLs, and every referenced script and stylesheet exists.
- Every `url(...)` in built CSS (fonts, images) resolves inside `dist/`.
- Manifest and icons linked from `index.html` exist.
- No source maps or `.env` contents are shipped unless the repository intends it.

## Match checks to risk

1. Run Oxlint during iteration.
2. Run `typecheck` when types, routes, search schemas, or API contracts changed and a build is not already planned.
3. Add or run Vitest + Testing Library only for high-risk interaction state that lint and types cannot falsify (selection logic, optimistic updates, search-param round-trips).
4. Run `vite build` when frontend inputs changed or before staging.
5. Run the combined artifact gate when the Go embedding seam changed or before release. It ends with the fallback smoke test against the built binary: `/`, a deep link with search params, a hashed JS and CSS asset (content type and `immutable`), a missing asset (`404`), `/api/unknown` (JSON `404`), `/api/config`, `/mcp`, and the manifest.
6. Use scripted Playwright only at the final regression gate, when changing its coverage, or when cheaper checks cannot falsify cross-app behavior. Include one deep-link reload in it.

Record warm wall times for standard checks. Measure clean baselines separately and infrequently. Never clear caches in the normal loop merely to produce a timing number.

## Guard staged correctness

- Fail `artifact` when the build or content fingerprint is absent or stale.
- Make backend-only builds identify the current staged UI and warn when stale without silently rebuilding it.
- Never cache secrets or allow undeclared environment values to affect the build; the UI reads no `VITE_*` application configuration.
- Provide an explicit diagnostic clean target without making it a routine dependency.

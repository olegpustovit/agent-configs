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
    "build": "vite build && tsc --noEmit",
    "preview": "vite preview"
  }
}
```

Run them with `bun run <script>`. `bun run test` runs Vitest; do not use `bun test`, which is Bun's own runner with different APIs. The router plugin regenerates `routeTree.gen.ts` during `dev` and `build`; when typechecking without a build, run the generator first (`tsr generate` from `@tanstack/router-cli`, or one `vite build`) so route types are current.

Configure Vitest in `vite.config.ts` (import `defineConfig` from `vitest/config`; `test: { environment: "jsdom", setupFiles: [...] }`) so tests share aliases and plugins. Render components under a test `QueryClientProvider` (retries off), a fresh Jotai `Provider`, and, for routed components, a memory-history router.

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

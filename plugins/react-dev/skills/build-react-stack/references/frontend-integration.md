# Same-origin frontend integration

Use this reference to connect the React SPA to the Go API without frontend deployment configuration, and to serve it from the Go binary with correct history-mode routing.

## Contents

- Vite configuration and development proxy
- Router, Query client, and providers
- API client, query keys, and mutations
- Public runtime configuration
- Auth and the URL-owned workspace
- Build, staging, and Go embedding
- SPA fallback contract and cache headers
- Browser identity
- PWA (opt-in): see [pwa.md](pwa.md)

## Configure Vite and proxy only in development

The Vite dev server (`:5173`) and the Go server (`:8080`, or the repository's fixed port) run separately in development. Browser code stays relative; Vite forwards API paths unchanged.

```ts
// ui/vite.config.ts
import path from "node:path"
import { defineConfig } from "vite"
import react, { reactCompilerPreset } from "@vitejs/plugin-react"
import babel from "@rolldown/plugin-babel"
import tailwindcss from "@tailwindcss/vite"
import { tanstackRouter } from "@tanstack/router-plugin/vite"

const goOrigin = "http://127.0.0.1:8080" // fixed, documented dev origin

export default defineConfig({
  base: "/",
  plugins: [
    tanstackRouter({ target: "react", autoCodeSplitting: true }), // before react()
    react(),
    babel({ presets: [reactCompilerPreset()] }), // React Compiler; see performance.md
    tailwindcss(),
  ],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "src") } },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: goOrigin },
      "/mcp": { target: goOrigin }, // only if the UI calls MCP
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
})
```

- Do not `rewrite` the proxied path; development and production must see the same `/api/*` paths.
- Leave `changeOrigin` off so the Go server sees the browser's host and cookies set without a `Domain` attribute bind to the dev origin. Add `ws: true` only for WebSocket endpoints; SSE works through the plain proxy.
- Vite's dev server already serves `index.html` for unknown paths, so deep links work in development. Production gets the same behavior only from the Go fallback below; test it there.
- Keep the Go origin in this file or the repository's dev tooling, not in `.env` files read by application code.

Primary sources: [Vite server options](https://vite.dev/config/server-options), [TanStack Router with Vite](https://tanstack.com/router/latest/docs/framework/react/installation/with-vite).

## Create the router, Query client, and providers once

```ts
// ui/src/router.tsx
import { QueryClient } from "@tanstack/react-query"
import { createRouter } from "@tanstack/react-router"
import { routeTree } from "./routeTree.gen"

export const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, retry: retryUnlessClientError } },
})

export const router = createRouter({
  routeTree,
  context: { queryClient },
  defaultPreload: "intent",
  defaultPreloadStaleTime: 0, // let Query decide freshness
  scrollRestoration: true,
})

declare module "@tanstack/react-router" {
  interface Register { router: typeof router }
}
```

```tsx
// ui/src/routes/__root.tsx
export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  loader: ({ context }) => context.queryClient.query({ ...configQuery, staleTime: "static" }),
  component: RootLayout,
  errorComponent: RootError,
  notFoundComponent: NotFound,
})
```

`main.tsx` renders `QueryClientProvider` → `TooltipProvider` → `RouterProvider router={router}` plus the toaster. Nothing else is global.

`queryClient.query(...)` is the current Query v5 API; with `staleTime: "static"` it replaces the deprecated `ensureQueryData` (return cached data, fetch only if absent). Use `ensureQueryData` only when the repository pins an older v5 that lacks `query`.

Primary sources: [Router context](https://tanstack.com/router/latest/docs/framework/react/guide/router-context), [External data loading](https://tanstack.com/router/latest/docs/framework/react/guide/external-data-loading), [Preloading with external libraries](https://tanstack.com/router/latest/docs/framework/react/guide/preloading).

## Keep browser requests relative and typed

One small client in `src/lib/api.ts` is the query-function boundary:

```ts
export async function api<T>(path: `/api/${string}`, init?: RequestInit, parse?: (v: unknown) => T): Promise<T> {
  const res = await fetch(path, { ...init, headers: { "Content-Type": "application/json", ...init?.headers } })
  if (!res.ok) throw await ApiError.from(res) // carries status + stable error code
  const body: unknown = res.status === 204 ? undefined : await res.json()
  return parse ? parse(body) : (body as T)
}
```

- Never build an origin from `window.location`, environment variables, or configuration. Relative URLs keep cookies and browser security same-origin.
- Validate responses at this boundary where the contract is not generated (zod or generated types from the API schema when the project already generates them).
- Do not retry 4xx responses; retry 5xx and network errors with Query's bounded backoff.

### Query keys and options

Define one `queryOptions` factory per resource in `src/features/<feature>/queries.ts` and use it in loaders, `useSuspenseQuery`, prefetching, and invalidation:

```ts
export const issueKeys = {
  all: (ws: string) => ["ws", ws, "issues"] as const,
  list: (ws: string, f: IssueFilters) => [...issueKeys.all(ws), "list", f] as const,
  detail: (ws: string, id: string) => [...issueKeys.all(ws), "detail", id] as const,
}

export const issueListQuery = (ws: string, f: IssueFilters) =>
  queryOptions({ queryKey: issueKeys.list(ws, f), queryFn: () => api<Issue[]>(`/api/workspaces/${ws}/issues?${toQuery(f)}`) })
```

Prefix every workspace-owned key with the workspace slug so switching workspaces cannot show another workspace's cache and `removeQueries({ queryKey: ["ws", old] })` clears it exactly.

### Loaders and mutations

```ts
export const Route = createFileRoute("/w/$workspace/issues/")({
  validateSearch: issueSearchSchema,
  loaderDeps: ({ search }) => ({ filters: search.filters }),
  loader: ({ context, params, deps }) =>
    context.queryClient.query({ ...issueListQuery(params.workspace, deps.filters), staleTime: "static" }),
  component: IssueList,
})
```

The component calls `useSuspenseQuery(issueListQuery(workspace, filters))`. Use `useMutation` for writes; on success write the authoritative response with `setQueryData` when small, otherwise `invalidateQueries` on the narrowest affected key. Keep form input local; do not mirror `isPending`, `error`, or data into state.

Primary sources: [queryOptions](https://tanstack.com/query/latest/docs/framework/react/guides/query-options), [useSuspenseQuery](https://tanstack.com/query/latest/docs/framework/react/reference/useSuspenseQuery), [useMutation](https://tanstack.com/query/latest/docs/framework/react/reference/useMutation).

## Serve public runtime configuration from Go

Expose one endpoint, `GET /api/config`, returning a dedicated, allowlisted response type, not the backend `Config` struct:

```json
{ "schema_version": 1, "features": { "new_editor": true }, "limits": { "upload_bytes": 10485760 } }
```

- Treat every field as public. Never expose secrets, Vault metadata, internal hostnames, database details, or authorization internals. User-specific settings belong in authenticated endpoints.
- Derive it from the final validated backend configuration at the transport boundary. Version the schema when compatibility matters.
- Default to `Cache-Control: no-store` so a new backend is never paired with stale configuration.
- Load it once in the root route loader through `configQuery` (`staleTime: Infinity`), read it with `useSuspenseQuery(configQuery)` where needed, and let the root `errorComponent` render bootstrap failure. Do not fall back to build-time defaults, and do not let components fetch it independently.

## Make auth and the URL workspace authoritative

- The workspace layout route `/w/$workspace` owns reconciliation in `beforeLoad`: load the session (`context.queryClient.query({ ...sessionQuery, staleTime: "static" })`), `throw redirect({ to: "/login", search: { redirect: location.href } })` when unauthenticated, and reject inaccessible slugs with `notFound()`.
- When the path slug differs from the session's current workspace, select it through the backend, remove the previous workspace's queries, refetch the principal, then continue. Child loaders run only after `beforeLoad` resolves, so no workspace-owned query mounts against the wrong workspace.
- Send the slug in every workspace API path and authorize it against membership at the backend; never rely only on a mutable current-workspace cookie, because another tab can change it.
- Render the workspace layout's `<Outlet />` with `key={workspace}` so per-workspace UI state resets on switch.
- On explicit switch, navigate to the new slug's path; drop entity params that belong to the old workspace.

## Build, stage, and embed

1. `bun run build` runs `vite build && tsc --noEmit` (the build regenerates `routeTree.gen.ts` first) and writes `ui/dist/` (`index.html`, hashed `assets/*`, copied `public/*`).
2. Verify the build: `index.html` references only `/assets/...` absolute URLs that exist; every `url(...)` in built CSS resolves; manifest and icons exist.
3. Copy to a temporary directory beside the embed target, compare, and replace `backend/internal/infrastructure/webui/dist/` (or the repository's equivalent) only when contents changed. Write the fingerprint last.

```go
// backend/internal/infrastructure/webui/webui.go
package webui

import (
	"embed"
	"io/fs"
)

//go:embed all:dist
var embedded embed.FS

// Dist returns the staged UI rooted at dist/.
func Dist() (fs.FS, error) { return fs.Sub(embedded, "dist") }
```

Keep a committed placeholder file in `dist/` so backend-only builds compile without a UI build; the complete artifact target, not compilation, enforces that the real UI is present and fresh.

## Own the SPA fallback contract

History-mode routing works only if the server answers deep links with `index.html` and answers everything else honestly. Register in this order in `cmd/api` routes (Fiber v3):

1. `/api/*` routes, then an `/api/*` catch-all returning the API's JSON `404`.
2. `/mcp`, health, metrics, and any other backend routes.
3. Hashed assets, then the fallback.

```go
func registerUI(app *fiber.App, dist fs.FS) error {
	index, err := fs.ReadFile(dist, "index.html")
	if err != nil {
		return fmt.Errorf("read embedded index.html: %w", err)
	}
	notFound := func(fiber.Ctx) error { return fiber.ErrNotFound }

	app.Get("/assets*", static.New("assets", static.Config{
		FS:              dist,
		NotFoundHandler: notFound,
		ModifyResponse: func(c fiber.Ctx) error {
			c.Set(fiber.HeaderCacheControl, "public, max-age=31536000, immutable")
			return nil
		},
	}))

	public := static.New("", static.Config{FS: dist, IndexNames: []string{}, MaxAge: 3600, NotFoundHandler: notFound})

	app.Get("/*", func(c fiber.Ctx) error {
		p := c.Path()
		switch {
		case isBackendPath(p): // /api, /mcp, /healthz, /metrics prefixes
			return fiber.ErrNotFound
		case p != "/index.html" && path.Ext(p) != "":
			return public(c) // favicon.ico, site.webmanifest, robots.txt, or 404
		}
		c.Set(fiber.HeaderCacheControl, "no-cache")
		c.Type("html")
		return c.Send(index)
	})
	return nil
}
```

The contract:

| Request | Response |
| --- | --- |
| `GET /`, `/w/acme/issues/42?tab=activity` (deep link or refresh) | `200`, `index.html`, `Cache-Control: no-cache` |
| `GET /assets/index-<hash>.js` / `.css` / `.woff2` | `200`, correct content type, `immutable` |
| `GET /assets/missing.js` | `404`, never HTML |
| `GET /favicon.ico`, `/site.webmanifest` | `200`, short cache |
| `GET /api/unknown` | API JSON `404`, never HTML |
| `/mcp`, `/healthz` | backend handler, never shadowed |

- If the product has app routes whose last segment can contain a dot, extend the HTML branch to `Accept: text/html` navigations instead of renaming routes.
- Assert the exact Fiber static options against the pinned Fiber version's docs; the contract table is what tests check.
- Put the table in a Go test using `app.Test` against a fixture `fs.FS` (`fstest.MapFS`), and run the same requests against the built binary in the artifact smoke test.

Primary source: [Fiber static middleware](https://docs.gofiber.io/middleware/static).

## Ship a complete browser identity

Generate the icon family from one canonical brand source with a repository script: SVG icon, `favicon.ico`, 16/32 px PNGs, 180 px Apple touch icon, 192/512 px PWA icons, a 512 px maskable icon, and `site.webmanifest` in `ui/public/`. Link them from `index.html` and set the document title per route with the router's `head` option and `HeadContent`. Remove Vite starter assets. Make the build verifier fail when a linked asset is missing, and probe the manifest and an icon through the Go binary, rejecting HTML bodies.

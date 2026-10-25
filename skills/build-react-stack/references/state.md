# Where state lives

Every piece of state has exactly one owner. Pick it with this table before writing a hook, atom, or param.

| State | Owner | Examples |
| --- | --- | --- |
| Data the Go API owns | **TanStack Query** cache | issues, the session principal, `/api/config`, counts |
| Identity of what is shown | **Path params** | `$workspace`, `$issueId` |
| Shareable / reloadable view state | **Typed search params** | filters, sort, page, tab, selected row, open "new issue" sheet, view mode |
| Ephemeral UI state shared across distant components | **Jotai atoms** | sidebar collapsed, command palette open, multi-select set, draft panel width, keyboard focus zone |
| Per-user conveniences that should survive reloads | `atomWithStorage` | preferred density, last-used view, dismissed hints |
| State used by one component or a parent and its children | **`useState` / `useReducer`** | form inputs, hover, an accordion, a controlled input |

Test: "Should this survive a reload or a copied link?" Yes → URL. "Does the server own it?" → Query. "Do two unrelated subtrees need it?" → atom. Otherwise local.

## TanStack Query owns server data

- Never copy query data into atoms, `useState`, or the URL. Derive with `select`, or compute during render.
- Never mirror `isPending` / `error` / `isFetching` into other state. Suspense and route `errorComponent` handle the common case; `useQuery` flags handle the rest.
- An atom may hold IDs that point into the cache (selected issue IDs), never the records themselves.
- Optimistic updates go through `useMutation` (`onMutate` + rollback, or the `variables` of a pending mutation), not through atoms.

## Search params own shareable view state

```ts
const issueSearchSchema = z.object({
  status: z.enum(["open", "done", "all"]).catch("open"),
  sort: z.enum(["updated", "created", "priority"]).catch("updated"),
  page: z.number().int().min(1).catch(1),
  selected: z.string().optional().catch(undefined),
})

export const Route = createFileRoute("/w/$workspace/issues/")({
  validateSearch: issueSearchSchema,                       // zod v4: pass directly
  search: { middlewares: [stripSearchParams({ status: "open", sort: "updated", page: 1 })] },
})

// read
const { status, page } = Route.useSearch()
// write
const navigate = Route.useNavigate()
navigate({ search: (prev) => ({ ...prev, page: prev.page + 1 }) })
```

- Validate every search param with a schema; never parse `URLSearchParams` by hand. Use `.catch()` so a malformed link degrades to defaults.
- Zod v4 and other Standard Schema libraries plug in directly; `@tanstack/zod-adapter` (`zodValidator`, `fallback`) is only for Zod v3.
- Strip defaults with `stripSearchParams` so URLs stay short; keep cross-route params (e.g. a global view mode) with `retainSearchParams`.
- Use `loaderDeps` to pass only the params a loader needs, so unrelated param changes do not refetch.
- Use `replace: true` for high-frequency updates (typing in a filter box after debounce) to avoid flooding history.
- Read params in deep children with `getRouteApi("/w/$workspace/issues/").useSearch()` instead of prop drilling.
- Overlays that should be linkable (create dialog, preview sheet) open from a search param; transient ones (a confirm dialog) stay local.

Primary source: [Search params guide](https://tanstack.com/router/latest/docs/framework/react/guide/search-params).

## Jotai owns small, ephemeral, cross-component UI state

Jotai's current major is v3 (ESM only, React 18+, TypeScript 5.5+). Its public hooks (`useAtom`, `useAtomValue`, `useSetAtom`) are unchanged from v2, but `atomFamily` moved to the `jotai-family` package and `loadable` was removed from `jotai/utils` (use `unwrap`). Check the installed major before importing.

```ts
// src/features/issues/atoms.ts
import { atom } from "jotai"
import { atomWithStorage } from "jotai/utils"
import { atomFamily } from "jotai-family" // v3; v2 had it in "jotai/utils"

export const selectedIssueIdsAtom = atom<ReadonlySet<string>>(new Set<string>())
export const selectedCountAtom = atom((get) => get(selectedIssueIdsAtom).size)      // derived
export const toggleIssueAtom = atom(null, (get, set, id: string) => {               // write-only action
  const next = new Set(get(selectedIssueIdsAtom))
  next.has(id) ? next.delete(id) : next.add(id)
  set(selectedIssueIdsAtom, next)
})
export const rowExpandedAtom = atomFamily((_id: string) => atom(false))
export const densityAtom = atomWithStorage<"compact" | "comfortable">("issues.density", "comfortable")
```

- Colocate atoms with the feature in `features/<feature>/atoms.ts`. No global `store.ts` of unrelated atoms.
- Keep atoms primitive and small; build the rest as derived read atoms and write-only action atoms. Components use `useAtomValue` / `useSetAtom` to subscribe to only what they need.
- Use `atomFamily` for per-entity UI state, and call `remove` (or `setShouldRemove`) when entities go away so the family does not grow without bound.
- Use `atomWithStorage` only for per-user conveniences. Never store server data, auth tokens, or anything security-relevant there.
- Do not put async fetching in atoms; that is Query's job. Do not use `jotai-tanstack-query` to duplicate the Query cache unless the repository already depends on it.
- Mount one `Provider` with an explicit `createStore()` at the root, and key a nested `Provider` by workspace slug when workspace-scoped UI state must reset on switch. Tests render with a fresh `Provider` per test (and `useHydrateAtoms` for initial values).
- If you are tempted to sync an atom with a search param, the state belongs in the search param.

Primary sources: [Jotai docs](https://jotai.org/docs), [Migrating to v3](https://github.com/pmndrs/jotai/blob/main/docs/guides/migrating-to-v3.mdx), [jotai-family](https://github.com/jotaijs/jotai-family).

## Local state stays local

- `useState` / `useReducer` for state one component (and its children through props) needs. Lift only as far as the nearest common parent; reach for an atom only when that parent is far away or unrelated.
- Derive values during render. An effect that sets state from props, query data, or search params is a bug.
- Use `useTransition` / `useDeferredValue` for expensive re-renders driven by input, not hand-rolled debounce state, unless a network request is being throttled.

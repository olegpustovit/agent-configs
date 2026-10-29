# Where state lives

Every piece of state has exactly one owner. Pick it with this table before writing a hook, context, or param. There is no client state library.

| State | Owner | Examples |
| --- | --- | --- |
| Data the Go API owns | **TanStack Query** cache | issues, the session principal, `/api/config`, counts |
| Identity of what is shown | **Path params** | `$workspace`, `$issueId` |
| Shareable / reloadable view state | **Typed search params** | filters, sort, page, tab, selected row, open "new issue" sheet, view mode |
| Ephemeral UI state shared across distant components | **`useState` in the nearest layout route**, passed down or through one context that layout owns | sidebar collapsed, command palette open, multi-select set |
| Per-user conveniences that should survive reloads | **`localStorage`** read once by the owning component (wrapped in `try`) | preferred density, dismissed hints |
| State used by one component or a parent and its children | **`useState` / `useReducer`** | form inputs, hover, an accordion, a controlled input |

Test: "Should this survive a reload or a copied link?" Yes → URL. "Does the server own it?" → Query. "Do two unrelated subtrees need it?" → the layout route that contains both. Otherwise local.

## TanStack Query owns server data

- Never copy query data into `useState`, context, or the URL. Derive with `select`, or compute during render.
- Never mirror `isPending` / `error` / `isFetching` into other state. Suspense and route `errorComponent` handle the common case; `useQuery` flags handle the rest.
- UI state may hold IDs that point into the cache (selected issue IDs), never the records themselves.
- Optimistic updates go through `useMutation` (`onMutate` + rollback, or the `variables` of a pending mutation), not through component state.

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

## Local state stays local

- `useState` / `useReducer` for state one component (and its children through props) needs. Lift only as far as the nearest common parent; when that parent is a layout route far away, expose the state through one context created in that layout. Do not add Jotai, Zustand, Redux, or a hand-written store.
- Derive values during render. An effect that sets state from props, query data, or search params is a bug.
- Use `useTransition` / `useDeferredValue` for expensive re-renders driven by input, not hand-rolled debounce state, unless a network request is being throttled.

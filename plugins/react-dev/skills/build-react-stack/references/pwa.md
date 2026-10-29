# Installable PWA (opt-in)

Add this only when the product asks for installability or offline start. A PWA here means: installable, starts instantly from the precached shell, and updates itself on the next load. API data is never served from the service worker cache; TanStack Query owns freshness.

## Generate the worker with vite-plugin-pwa

Install `vite-plugin-pwa` and let it generate the Workbox service worker. Do not hand-write `sw.js`, and do not add a second build entry for it.

```ts
// ui/vite.config.ts (add to plugins, after tailwindcss())
import { VitePWA } from "vite-plugin-pwa"

VitePWA({
  registerType: "autoUpdate",
  injectRegister: "script-defer",
  manifest: false, // public/site.webmanifest stays the single source; see frontend-integration.md
  workbox: {
    globPatterns: ["**/*.{js,css,html,svg,png,ico,woff2}"],
    navigateFallback: "/index.html",
    navigateFallbackDenylist: [/^\/api\//, /^\/mcp/, /^\/healthz/, /^\/metrics/],
    cleanupOutdatedCaches: true,
  },
})
```

- `navigateFallback` lets deep links open offline; the denylist keeps backend paths on the network so OAuth redirects, MCP, and API errors are never answered with the cached shell.
- Add no `runtimeCaching` for `/api`. Add it for a specific resource only when the product asks for offline data, and then with `NetworkFirst` and a short expiry.
- Lazy route chunks are precached too, so watch the budget in [performance.md](performance.md): every precached byte is downloaded on install.
- Add `"display": "standalone"`, `"start_url": "/"`, `"scope": "/"`, `theme_color`, `background_color`, and the 192/512/maskable icons to `public/site.webmanifest`.
- Need an "update available" prompt instead of silent updates? Switch to `registerType: "prompt"` with `useRegisterSW` from `virtual:pwa-register/react` and show a toast. Do this only when the product asks.

## Serve the worker from Go without caching it

The browser checks `sw.js` for updates on navigation; a cached worker pins users to an old build. Add these to the SPA fallback before the `public(c)` branch:

```go
noCache := static.New("", static.Config{FS: dist, IndexNames: []string{}, NotFoundHandler: notFound})

// inside app.Get("/*", ...)
case p == "/sw.js" || p == "/registerSW.js" || p == "/site.webmanifest":
	c.Set(fiber.HeaderCacheControl, "no-cache")
	return noCache(c)
```

`workbox-<hash>.js` is content-hashed and may keep the short public cache.

Extend the contract table and its Go test:

| Request | Response |
| --- | --- |
| `GET /sw.js` | `200`, `text/javascript`, `Cache-Control: no-cache` |
| `GET /site.webmanifest` | `200`, `application/manifest+json`, `no-cache` |
| `GET /api/unknown` with a service worker active | API JSON `404`, never the cached shell |

## Verify

- `dist/sw.js` exists and lists `index.html` and the hashed entry assets in its precache manifest.
- Through the built Go binary, the smoke test checks the two `no-cache` rows above.
- In one Playwright run: load `/`, wait for `navigator.serviceWorker.ready`, go offline with `context.setOffline(true)`, reload a deep link, and expect the app shell. Skip this check for non-PWA apps.

Primary source: [vite-plugin-pwa guide](https://vite-pwa-org.netlify.app/guide/).

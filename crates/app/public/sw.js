// Billo's service worker: loads the app shell with no network. API responses are never
// cached (the app keeps its data in IndexedDB and syncs it itself).
//
// Strategy: cache at run time, so it works with whatever file names `dx build` makes
// (hashed `-dxh…` assets in release builds, plain names in debug builds):
// - Page loads (navigations): network first, falling back to the cached page.
// - Hashed files (`-dxh<hex>`): cache first; a new build has new names.
// - Other files of this site (wasm, js, icons, manifest): network first, cached copy
//   when offline.
// - The page sends the URLs it loaded before this worker took control, so one online
//   visit is enough to work offline.
// - Other origins (the API server) and non-GET requests are left alone.
//
// The cache is named after the app version and build (`sw.js?v=0.1.0-1`): registering a
// new release installs a new worker, which deletes the old caches.

const VERSION = new URL(self.location.href).searchParams.get("v") || "dev";
const PREFIX = "billo-shell-";
const CACHE = PREFIX + VERSION;
const SHELL = ["/", "/manifest.webmanifest", "/icon-192.png", "/icon-512.png"];
const HASHED = /-dxh[0-9a-f]+\./;
// Dev servers answer with `Vary: Origin`; module scripts send an Origin header and the
// copies cached from the page's list don't, so match without it (same-origin files
// don't really vary).
const MATCH = { ignoreVary: true };

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((k) => k.startsWith(PREFIX) && k !== CACHE)
            .map((k) => caches.delete(k)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

function cacheable(url) {
  return url.origin === self.location.origin && !url.pathname.startsWith("/api/");
}

// URLs the page loaded before this worker controlled it.
self.addEventListener("message", (event) => {
  const data = event.data || {};
  if (data.type !== "cache-urls" || !Array.isArray(data.urls)) {
    return;
  }
  const urls = data.urls
    .map((u) => new URL(u, self.location.href))
    .filter(cacheable)
    .map((u) => u.href);
  event.waitUntil(
    caches.open(CACHE).then((cache) =>
      Promise.all(
        urls.map((u) =>
          cache.match(u, MATCH).then((hit) => hit || cache.add(u).catch(() => undefined)),
        ),
      ),
    ),
  );
});

async function networkFirst(request, fallback) {
  const cache = await caches.open(CACHE);
  try {
    const response = await fetch(request);
    if (response.ok) {
      cache.put(fallback || request, response.clone());
    }
    return response;
  } catch (err) {
    const hit = await cache.match(fallback || request, { ...MATCH, ignoreSearch: !!fallback });
    if (hit) {
      return hit;
    }
    throw err;
  }
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request, MATCH);
  if (hit) {
    return hit;
  }
  const response = await fetch(request);
  if (response.ok) {
    cache.put(request, response.clone());
  }
  return response;
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") {
    return;
  }
  const url = new URL(request.url);
  if (!cacheable(url)) {
    return;
  }
  if (request.mode === "navigate") {
    // Every page of this single-page app is the same shell.
    event.respondWith(networkFirst(request, "/"));
  } else if (HASHED.test(url.pathname)) {
    event.respondWith(cacheFirst(request));
  } else {
    event.respondWith(networkFirst(request));
  }
});

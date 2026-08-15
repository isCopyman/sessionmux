export const SESSION_WARM_CACHE_LIMIT_STORAGE_KEY =
  "settings:session-warm-cache-limit:v1"
export const SESSION_WARM_CACHE_LIMIT_CHANGED_EVENT =
  "codeg:session-warm-cache-limit-changed"
export const DEFAULT_SESSION_WARM_CACHE_LIMIT = 8
export const MIN_SESSION_WARM_CACHE_LIMIT = 0
export const MAX_SESSION_WARM_CACHE_LIMIT = 50

export function normalizeSessionWarmCacheLimit(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value)
  if (!Number.isFinite(parsed)) return DEFAULT_SESSION_WARM_CACHE_LIMIT
  return Math.max(
    MIN_SESSION_WARM_CACHE_LIMIT,
    Math.min(MAX_SESSION_WARM_CACHE_LIMIT, Math.round(parsed))
  )
}

export function readSessionWarmCacheLimit(): number {
  if (typeof window === "undefined") return DEFAULT_SESSION_WARM_CACHE_LIMIT
  const stored = window.localStorage.getItem(
    SESSION_WARM_CACHE_LIMIT_STORAGE_KEY
  )
  return stored == null
    ? DEFAULT_SESSION_WARM_CACHE_LIMIT
    : normalizeSessionWarmCacheLimit(stored)
}

export function writeSessionWarmCacheLimit(value: unknown): number {
  const limit = normalizeSessionWarmCacheLimit(value)
  if (typeof window === "undefined") return limit
  window.localStorage.setItem(
    SESSION_WARM_CACHE_LIMIT_STORAGE_KEY,
    String(limit)
  )
  window.dispatchEvent(
    new CustomEvent<number>(SESSION_WARM_CACHE_LIMIT_CHANGED_EVENT, {
      detail: limit,
    })
  )
  return limit
}

/** Listen in the current WebView and across sibling settings/workspace windows. */
export function subscribeSessionWarmCacheLimit(
  callback: (limit: number) => void
): () => void {
  if (typeof window === "undefined") return () => {}
  const onCustom = (event: Event) => {
    const detail = (event as CustomEvent<number>).detail
    callback(normalizeSessionWarmCacheLimit(detail))
  }
  const onStorage = (event: StorageEvent) => {
    if (event.key !== SESSION_WARM_CACHE_LIMIT_STORAGE_KEY) return
    callback(readSessionWarmCacheLimit())
  }
  callback(readSessionWarmCacheLimit())
  window.addEventListener(SESSION_WARM_CACHE_LIMIT_CHANGED_EVENT, onCustom)
  window.addEventListener("storage", onStorage)
  return () => {
    window.removeEventListener(SESSION_WARM_CACHE_LIMIT_CHANGED_EVENT, onCustom)
    window.removeEventListener("storage", onStorage)
  }
}

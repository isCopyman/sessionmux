"use client"

import { useEffect } from "react"

// Opt-out hatch: the target or any ancestor carrying this attribute keeps the
// browser's own menu.
const NATIVE_MENU_OPT_OUT = "[data-native-context-menu]"

// `contenteditable="false"` marks a NON-editable island inside an editable
// host, so it must not match on its own.
const EDITABLE_SELECTOR =
  'input, textarea, [contenteditable]:not([contenteditable="false"])'

function keepsNativeMenu(target: Element): boolean {
  if (target.closest(NATIVE_MENU_OPT_OUT)) return true
  if (target.closest(EDITABLE_SELECTOR)) return true
  // Editability can also be inherited without the attribute ever appearing in
  // the ancestor chain (`document.designMode`, or a host set through the IDL
  // property), which the selector walk above cannot see.
  return target instanceof HTMLElement && target.isContentEditable
}

function suppressNativeMenu(event: MouseEvent): void {
  // Dev-only inspector hatch. `process.env.NODE_ENV` is inlined at build time,
  // so the static export (`next build`, always "production") drops this branch
  // entirely while `next dev` keeps it.
  if (process.env.NODE_ENV !== "production" && event.shiftKey) return
  const target = event.target
  if (target instanceof Element && keepsNativeMenu(target)) return
  event.preventDefault()
}

/**
 * Cancels the WebView's own context menu (Back / Reload / Save as / Print /
 * Inspect) app-wide. Those entries are meaningless in a desktop shell and
 * "Back" can navigate the SPA out from under itself. Mounted once from the
 * root layout, so every route is covered — `/pet` and `/settings` included.
 *
 * Only the DEFAULT ACTION is cancelled; propagation is never touched, so every
 * component-level `onContextMenu` (Radix triggers included) still runs and
 * every custom menu still opens. Exempt: editable targets, where the native
 * menu carries real copy/paste/spellcheck, and `data-native-context-menu`
 * opt-outs.
 */
export function NativeContextMenuGuard() {
  useEffect(() => {
    // Bubble phase on `document`, never capture. Radix's context-menu trigger
    // composes its opener with `checkForDefaultPrevented`, so a capture-phase
    // `preventDefault()` here would arrive first and make every custom menu in
    // the app refuse to open. Bubbling to `document` runs last instead — and
    // React delegates its synthetic handlers to `document` too (App Router
    // hydrates the document node), so a component's `stopPropagation()`, fired
    // while the event is already AT `document`, cannot skip this listener.
    document.addEventListener("contextmenu", suppressNativeMenu)
    return () => {
      document.removeEventListener("contextmenu", suppressNativeMenu)
    }
  }, [])

  return null
}

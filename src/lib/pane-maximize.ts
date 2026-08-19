import { leafIds, type LayoutNode } from "./tab-group-layout"

/**
 * Decide whether a temporarily-maximized pane ("zoomed", tmux/VS Code style)
 * should auto-restore.
 *
 * `maximizedGroupId` (see the tab store) is a pure view-layer focus flag — it
 * never rewrites `groupLayout`. Whenever a real layout-level operation DOES
 * touch the tree (a split, a dissolve/merge, an orientation flip, or a remote
 * layout sync landing from another window/device) the maximized id can go
 * stale, so ANY structural change (a new `groupLayout` reference) clears it
 * unconditionally — regardless of whether the id happens to still name a
 * leaf. It also clears when the active tab's own group stops matching the
 * maximized one: next/prev-tab, a remote tab-switch, or any other focus move
 * that lands on a different, currently-hidden pane, since leaving the flag
 * set would then hide the very pane the user is focused on.
 *
 * Deliberately does NOT clear just because some background (non-active)
 * group's tab count or contents change: a hidden pane may keep working while
 * zoomed, exactly like a zoomed tmux pane leaves its neighbours running.
 */
export function shouldExitMaximizedGroup({
  maximizedGroupId,
  prevGroupLayout,
  groupLayout,
  activeGroupId,
}: {
  maximizedGroupId: string | null
  prevGroupLayout: LayoutNode
  groupLayout: LayoutNode
  /** The ACTIVE tab's current group id (resolve with `groupOfTab`), or null
   *  when there is no active tab. */
  activeGroupId: string | null
}): boolean {
  if (!maximizedGroupId) return false
  if (groupLayout !== prevGroupLayout) return true
  if (!leafIds(groupLayout).includes(maximizedGroupId)) return true
  // No active tab is no signal, not a mismatch — transient nulls during tab
  // switches/closes must not yank the zoom.
  return activeGroupId !== null && activeGroupId !== maximizedGroupId
}

/**
 * Gate for the maximized-pane Escape-to-restore shortcut: only a plain,
 * not-already-handled Escape with no dialog/menu open and focus outside an
 * editable element should restore the layout — otherwise Escape must fall
 * through to whatever overlay or input already owns it (closing a dropdown,
 * canceling an inline edit, etc.), matching the rest of the app's Escape
 * handling (see e.g. `useSessionMultiSelect`).
 */
export function isMaximizeRestoreEscape(
  event: Pick<KeyboardEvent, "key" | "defaultPrevented">,
  context: { hasOpenOverlay: boolean; focusIsEditable: boolean }
): boolean {
  if (event.key !== "Escape" || event.defaultPrevented) return false
  return !context.hasOpenOverlay && !context.focusIsEditable
}

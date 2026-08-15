"use client"

import { createPortal } from "react-dom"
import { useTabStore } from "@/contexts/tab-context"

/**
 * Floating chip that follows the pointer for the whole conversation-tab drag.
 * The source tab remains in its original slot as a dimmed placeholder; this
 * duplicate is the only visual that moves, matching VS Code and Paseo. It is
 * therefore visible before a pane target is acquired as well as over join and
 * edge-split targets.
 *
 * Portal to <body>: ancestors animate with transforms, which would re-anchor
 * `position: fixed` to themselves instead of the viewport.
 *
 * Text-selection suppression during a drag is NOT here: it belongs to every tab
 * drag (within-group sorting, the unsplit strip), not just the pane moves
 * that produce a ghost, so `TabItem` holds it via `drag-selection-guard`.
 */
export function TabDragGhost() {
  const drag = useTabStore((s) => s.tabDrag)
  if (!drag) return null
  return createPortal(
    <div
      aria-hidden
      data-tab-drag-ghost
      className="pointer-events-none fixed z-[100] flex max-w-56 items-center rounded-md border border-border bg-background/95 px-2.5 py-1 text-xs text-foreground shadow-md"
      style={{ left: drag.x + 10, top: drag.y + 12 }}
    >
      <span className="truncate">{drag.title}</span>
    </div>,
    document.body
  )
}

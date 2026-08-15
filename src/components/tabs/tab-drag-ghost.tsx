"use client"

import { createPortal } from "react-dom"
import { useTabStore } from "@/contexts/tab-context"

/**
 * Floating chip that follows the pointer while a conversation tab has a live
 * pane drop target. The dragged tab itself is axis-locked inside its own strip
 * (Reorder `drag="x"` + overflow clipping), so this ghost is the only visual
 * that crosses pane boundaries. It appears for both an existing-pane move and
 * an edge-split target, including an edge of the source pane.
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
  if (!drag || drag.overGroupId == null) return null
  return createPortal(
    <div
      aria-hidden
      className="pointer-events-none fixed z-[100] flex max-w-56 items-center rounded-md border border-border bg-background/95 px-2.5 py-1 text-xs text-foreground shadow-md"
      style={{ left: drag.x + 10, top: drag.y + 12 }}
    >
      <span className="truncate">{drag.title}</span>
    </div>,
    document.body
  )
}

/**
 * Pure math for cross-group tab drag & drop. The DOM side (hit-testing the
 * strip/shell under the pointer) lives in the tab bar; this module only turns
 * measured geometry into a drop index and normalizes pointer coordinates, so
 * both are unit-testable without a DOM.
 */

/**
 * Insertion index for a drop at `clientX` given the ascending midpoints of the
 * target strip's tabs: the tab lands before the first tab whose midpoint is to
 * the right of the cursor. (The app renders LTR in every locale, so a plain
 * left-to-right count is correct.)
 */
export function dropIndexFromMidpoints(
  clientX: number,
  midpoints: number[]
): number {
  let index = 0
  for (const mid of midpoints) {
    if (mid < clientX) index += 1
  }
  return index
}

/**
 * Apply a tab-strip insertion only when the drag is released. The DOM drop
 * index still counts the dragged tab's original slot, so moving right must
 * discount that slot after removing the tab. Keeping this calculation pure
 * lets the strip retain its original order while a floating drag ghost moves,
 * then commit one authoritative order at drop time (VS Code/Paseo behavior).
 */
export function moveIdToDropIndex(
  ids: string[],
  draggedId: string,
  dropIndex: number
): string[] {
  const sourceIndex = ids.indexOf(draggedId)
  if (sourceIndex < 0) return ids
  const without = ids.filter((id) => id !== draggedId)
  const insertionIndex = insertionIndexAfterRemovingSource(
    ids,
    draggedId,
    dropIndex
  )
  return [
    ...without.slice(0, insertionIndex),
    draggedId,
    ...without.slice(insertionIndex),
  ]
}

/**
 * Normalize a DOM drop index (which still counts the stationary source tab) to
 * an insertion boundary in the remaining tabs. The tab strip uses this same
 * value for its visual placeholder and the final commit, so the marker cannot
 * promise one slot and then land in another.
 */
export function insertionIndexAfterRemovingSource(
  ids: string[],
  draggedId: string,
  dropIndex: number
): number {
  const sourceIndex = ids.indexOf(draggedId)
  if (sourceIndex < 0) return -1
  const adjustedIndex = sourceIndex < dropIndex ? dropIndex - 1 : dropIndex
  return Math.max(0, Math.min(adjustedIndex, ids.length - 1))
}

export interface DragClientPoint {
  x: number
  y: number
}

/**
 * Center of a drag item's translated rectangle. Motion exposes the pointer's
 * displacement from drag start, while dnd-kit (and Paseo) resolves pane drops
 * from the translated item's center. Keeping that geometry here makes the two
 * drag implementations equivalent without coupling the tab bar to either
 * library's rectangle types.
 */
export function translatedRectCenter(
  rect: ClientRectLike,
  offset: DragClientPoint
): DragClientPoint {
  return {
    x: rect.left + rect.width / 2 + offset.x,
    y: rect.top + rect.height / 2 + offset.y,
  }
}

export type SplitDropEdge = "left" | "right" | "up" | "down"

export interface ClientRectLike {
  left: number
  top: number
  width: number
  height: number
}

export interface SplitDropGeometry {
  /** The centered rectangle that means "join this pane". */
  centerRatio?: number
  /** Once an edge is active, move this far into the center before leaving it. */
  hysteresisRatio?: number
  currentEdge?: SplitDropEdge | null
}

/**
 * Resolve the pane edge targeted by a tab drag. Like Paseo's split container,
 * the middle 40% x 40% is the only ordinary pane-join target; the surrounding
 * area belongs to the nearest split direction. This is much easier to acquire
 * than four thin edge strips, especially inside a narrow nested pane.
 *
 * A small hysteresis keeps an already-active edge selected until the pointer
 * has moved clearly into the center, preventing preview flicker at 30%/70%.
 */
export function splitDropEdgeFromPoint(
  clientX: number,
  clientY: number,
  rect: ClientRectLike,
  geometry: SplitDropGeometry = {}
): SplitDropEdge | null {
  if (
    !Number.isFinite(rect.width) ||
    !Number.isFinite(rect.height) ||
    rect.width <= 0 ||
    rect.height <= 0
  ) {
    return null
  }
  const x = (clientX - rect.left) / rect.width
  const y = (clientY - rect.top) / rect.height
  if (x < 0 || x > 1 || y < 0 || y > 1) return null

  const centerRatio = Math.min(Math.max(geometry.centerRatio ?? 0.4, 0), 1)
  const hysteresis = geometry.currentEdge
    ? Math.min(Math.max(geometry.hysteresisRatio ?? 0.04, 0), centerRatio / 2)
    : 0
  const centerInset = (1 - centerRatio) / 2 + hysteresis
  if (
    x >= centerInset &&
    x <= 1 - centerInset &&
    y >= centerInset &&
    y <= 1 - centerInset
  ) {
    return null
  }

  const candidates: Array<{ edge: SplitDropEdge; distance: number }> = [
    { edge: "left", distance: x },
    { edge: "right", distance: 1 - x },
    { edge: "up", distance: y },
    { edge: "down", distance: 1 - y },
  ]
  candidates.sort((a, b) => a.distance - b.distance)
  return candidates[0]?.edge ?? geometry.currentEdge ?? null
}

/**
 * Client (viewport) coordinates of a motion drag event. Motion hands back the
 * original event plus `PanInfo` whose `point` is in PAGE coordinates — prefer
 * the event's own client coords (pointer/mouse), fall back to page − scroll
 * (touch events carry coords per touch, not on the event itself).
 */
export function clientPointFromDrag(
  event: unknown,
  info: { point: { x: number; y: number } }
): DragClientPoint {
  const maybe = event as { clientX?: unknown; clientY?: unknown } | null
  if (
    maybe != null &&
    typeof maybe.clientX === "number" &&
    typeof maybe.clientY === "number"
  ) {
    return { x: maybe.clientX, y: maybe.clientY }
  }
  return {
    x: info.point.x - (typeof window !== "undefined" ? window.scrollX : 0),
    y: info.point.y - (typeof window !== "undefined" ? window.scrollY : 0),
  }
}

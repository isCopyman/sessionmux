/**
 * Pure IME-composition logic for the `@` mention panel, extracted so the
 * composition sequence can be unit-tested without driving a real ProseMirror
 * view (jsdom cannot emulate an input method). Wired up in
 * `mention-suggestion.ts`.
 *
 * Why any of this is needed: `@tiptap/suggestion` re-runs its matcher on every
 * transaction, and a CJK composition produces a stream of them carrying
 * half-finished text — `@美shu` on the way to `@美术`, and on some engines a
 * fragment split into its own text node, which the plugin's matcher (it reads
 * `$position.nodeBefore` only) cannot see the `@` from at all. Left alone, that
 * stream either searches for romaji nobody typed or reports the trigger as gone
 * and tears the panel down mid-word.
 *
 * So the panel is *frozen* for the length of the composition. The plugin itself
 * keeps running — its decoration has to stay where it is, since removing it
 * would rewrite the very DOM the IME is composing in — but nothing the plugin
 * reports reaches React until the composition commits. At that point the panel
 * is reconciled exactly once against the plugin's settled state, which is the
 * only state that describes text the user actually chose.
 */

import type { MentionRenderState } from "./mention-suggestion"

/** A lifecycle report from the suggestion plugin, in controller terms. */
export type MentionGateEvent =
  | { type: "start"; state: MentionRenderState }
  | { type: "update"; state: MentionRenderState }
  | { type: "exit" }

/** What the controller should be told. Same shape as what came in. */
export type MentionGateCall = MentionGateEvent

export interface MentionGate {
  /** A composition is in flight, so plugin reports are held back. */
  readonly composing: boolean
  /** Whether the controller has been told the panel is open. */
  readonly open: boolean
}

export const INITIAL_MENTION_GATE: MentionGate = {
  composing: false,
  open: false,
}

export interface MentionGateResult {
  gate: MentionGate
  /** The single call to forward, or null to leave the panel as it is. */
  call: MentionGateCall | null
}

/**
 * Whether the suggestion plugin may treat the current match as a live trigger —
 * the `allow` option of `@tiptap/suggestion`.
 *
 * `isActive` is the plugin's own `prev.active`: a panel is already open for this
 * trigger. That distinction is the whole answer to the IME question. Returning a
 * flat `false` while composing (what this used to do) makes the plugin drop the
 * suggestion on the first composing transaction, which fires `onExit` and closes
 * the panel two characters into a Chinese word. Returning a flat `true` instead
 * lets a composition *start* a trigger from text the user has not committed yet.
 * So: keep an already-open trigger alive through the composition, and let a new
 * one wait for the composition to commit (the reconcile in
 * {@link gateCompositionEnd} opens it a moment later, from settled text).
 */
export function shouldAllowMention(input: {
  /** A composition is in flight (ProseMirror's `view.composing`, widened). */
  composing: boolean
  /** The plugin already has a live suggestion for this trigger. */
  isActive: boolean
  /** The caret sits in a code block, where `@` is never a mention. */
  inCodeBlock: boolean
}): boolean {
  if (input.inCodeBlock) return false
  if (input.composing) return input.isActive
  return true
}

/** A composition started: freeze the panel where it is. */
export function gateCompositionStart(gate: MentionGate): MentionGate {
  return { ...gate, composing: true }
}

/**
 * Route one plugin report. While frozen nothing is forwarded: the intermediate
 * query must not reach the search (it is romaji, or a partial word), and an exit
 * caused by a momentarily unmatchable document must not close a panel the user
 * is still typing into.
 */
export function gateSuggestionEvent(
  gate: MentionGate,
  event: MentionGateEvent
): MentionGateResult {
  if (gate.composing) return { gate, call: null }
  switch (event.type) {
    case "start":
      return { gate: { ...gate, open: true }, call: event }
    case "update":
      return { gate, call: event }
    case "exit":
      // Nothing open means nothing to close — the plugin exits for reasons the
      // panel never saw (a trigger that was disallowed from the start).
      return { gate: { ...gate, open: false }, call: gate.open ? event : null }
  }
}

/**
 * Lift the freeze without reconciling anything yet.
 *
 * This is a separate step because of the order the reconcile has to run in: the
 * suggestion plugin is made to re-match the settled document *before* its state
 * can be read back, and its `allow` consults this same flag. A suggestion the
 * composition knocked out (the split-text-node case) would be refused entry
 * again — and stay dead — if the freeze were still on for that pass.
 */
export function gateCompositionSettled(gate: MentionGate): MentionGate {
  return { ...gate, composing: false }
}

/**
 * The composition committed (or was cancelled): reconcile the panel against
 * `live` — the suggestion plugin's state read back after it has been made to
 * re-match the settled document.
 *
 * The plugin's own events cannot be relied on here. The composed text often
 * lands in the document *during* the composition, so by the time it ends the
 * plugin may already hold the final query and have no change left to report,
 * while React is still showing the query from before the composition started.
 * Reading the settled state and pushing it once covers that case and the ones
 * where the plugin does emit, at the cost of a redundant refresh in the latter
 * (same query in, so the panel re-renders but does not re-search).
 *
 * A cancelled composition (Escape) needs no special handling: the IME reverts
 * the document itself, so `live` simply carries the pre-composition query again.
 */
export function gateCompositionEnd(
  gate: MentionGate,
  live: MentionRenderState | null
): MentionGateResult {
  const next: MentionGate = { composing: false, open: live !== null }
  if (live) {
    return {
      gate: next,
      call: { type: gate.open ? "update" : "start", state: live },
    }
  }
  return { gate: next, call: gate.open ? { type: "exit" } : null }
}

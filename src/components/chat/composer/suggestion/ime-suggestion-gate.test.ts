import { describe, expect, it } from "vitest"

import {
  gateCompositionEnd,
  gateCompositionSettled,
  gateCompositionStart,
  gateSuggestionEvent,
  shouldAllowMention,
  INITIAL_MENTION_GATE,
  type MentionGate,
  type MentionGateResult,
} from "./ime-suggestion-gate"
import type { MentionRenderState } from "./mention-suggestion"

const renderState = (query: string): MentionRenderState => ({
  query,
  range: { from: 1, to: 2 + query.length },
  getClientRect: null,
})

/** A panel the controller has been told about, with no composition running. */
const OPEN: MentionGate = { composing: false, open: true }

describe("shouldAllowMention", () => {
  it("allows a trigger outside a composition", () => {
    expect(
      shouldAllowMention({
        composing: false,
        isActive: false,
        inCodeBlock: false,
      })
    ).toBe(true)
  })

  it("keeps an already-open panel allowed while composing", () => {
    // Regression guard for #518. A flat `false` here — what this used to
    // return — makes @tiptap/suggestion drop the suggestion on the first
    // transaction of the composition, which fires onExit and closes the panel
    // on the second character of a Chinese word.
    expect(
      shouldAllowMention({
        composing: true,
        isActive: true,
        inCodeBlock: false,
      })
    ).toBe(true)
  })

  it("does not let a composition open a brand new trigger", () => {
    expect(
      shouldAllowMention({
        composing: true,
        isActive: false,
        inCodeBlock: false,
      })
    ).toBe(false)
  })

  it("never triggers inside a code block, composing or not", () => {
    expect(
      shouldAllowMention({
        composing: false,
        isActive: true,
        inCodeBlock: true,
      })
    ).toBe(false)
    expect(
      shouldAllowMention({
        composing: true,
        isActive: true,
        inCodeBlock: true,
      })
    ).toBe(false)
  })
})

describe("mention composition gate", () => {
  it("forwards plugin events untouched outside a composition", () => {
    const started = gateSuggestionEvent(INITIAL_MENTION_GATE, {
      type: "start",
      state: renderState("do"),
    })
    expect(started.call).toEqual({ type: "start", state: renderState("do") })
    expect(started.gate.open).toBe(true)

    const updated = gateSuggestionEvent(started.gate, {
      type: "update",
      state: renderState("doc"),
    })
    expect(updated.call).toEqual({ type: "update", state: renderState("doc") })

    const exited = gateSuggestionEvent(updated.gate, { type: "exit" })
    expect(exited.call).toEqual({ type: "exit" })
    expect(exited.gate.open).toBe(false)
  })

  it("holds back the intermediate query while composing", () => {
    const gate = gateCompositionStart(OPEN)
    const result = gateSuggestionEvent(gate, {
      type: "update",
      state: renderState("美shu"),
    })
    expect(result.call).toBeNull()
    expect(result.gate.open).toBe(true)
  })

  it("keeps the panel open when the plugin exits mid-composition", () => {
    const gate = gateCompositionStart(OPEN)
    const result = gateSuggestionEvent(gate, { type: "exit" })
    expect(result.call).toBeNull()
    expect(result.gate.open).toBe(true)
  })

  it("refreshes an open panel with the committed query", () => {
    const gate = gateCompositionStart(OPEN)
    const result = gateCompositionEnd(gate, renderState("美术"))
    expect(result.call).toEqual({
      type: "update",
      state: renderState("美术"),
    })
    expect(result.gate).toEqual({ composing: false, open: true })
  })

  it("opens the panel when the composition produced a fresh trigger", () => {
    const gate = gateCompositionStart(INITIAL_MENTION_GATE)
    const result = gateCompositionEnd(gate, renderState("美术"))
    expect(result.call).toEqual({ type: "start", state: renderState("美术") })
    expect(result.gate.open).toBe(true)
  })

  it("closes the panel when the composition left no trigger", () => {
    const gate = gateCompositionStart(OPEN)
    const result = gateCompositionEnd(gate, null)
    expect(result.call).toEqual({ type: "exit" })
    expect(result.gate).toEqual({ composing: false, open: false })
  })

  it("stays closed when a composition ends with no trigger", () => {
    const gate = gateCompositionStart(INITIAL_MENTION_GATE)
    const result = gateCompositionEnd(gate, null)
    expect(result.call).toBeNull()
    expect(result.gate).toEqual({ composing: false, open: false })
  })

  it("drops a redundant exit for a panel that was never opened", () => {
    const result = gateSuggestionEvent(INITIAL_MENTION_GATE, { type: "exit" })
    expect(result.call).toBeNull()
  })

  it("lets the plugin through again once the freeze is lifted", () => {
    // The recompute that produces the settled state runs between these two
    // steps, so anything the plugin reports during it must be forwarded — and
    // `shouldAllowMention` must see a gate that is no longer composing.
    const gate = gateCompositionSettled(gateCompositionStart(OPEN))
    expect(gate.composing).toBe(false)
    expect(gate.open).toBe(true)
    const result = gateSuggestionEvent(gate, {
      type: "update",
      state: renderState("美术"),
    })
    expect(result.call).toEqual({ type: "update", state: renderState("美术") })
  })

  it("keeps one panel alive across the @美 → @美术 sequence (#518)", () => {
    const forwarded: string[] = []
    let gate = INITIAL_MENTION_GATE
    const run = (result: MentionGateResult) => {
      gate = result.gate
      const call = result.call
      if (!call) return
      forwarded.push(
        call.type === "exit" ? "exit" : `${call.type}:${call.state.query}`
      )
    }

    // `@美` is typed and confirmed: the panel opens on the committed query.
    run(gateSuggestionEvent(gate, { type: "start", state: renderState("美") }))

    // The second character is composed. Every report in between is either
    // romaji nobody typed or a match the split composition text broke, and none
    // of it may reach the panel.
    gate = gateCompositionStart(gate)
    run(
      gateSuggestionEvent(gate, {
        type: "update",
        state: renderState("美shu"),
      })
    )
    run(gateSuggestionEvent(gate, { type: "exit" }))
    run(
      gateSuggestionEvent(gate, {
        type: "update",
        state: renderState("美术"),
      })
    )
    expect(gate.open).toBe(true)

    // `术` lands. The freeze comes off, the plugin is re-matched against the
    // settled document (nothing to forward — it already holds this query), and
    // the panel is reconciled once.
    gate = gateCompositionSettled(gate)
    run(gateCompositionEnd(gate, renderState("美术")))

    expect(forwarded).toEqual(["start:美", "update:美术"])
    expect(gate).toEqual({ composing: false, open: true })
  })

  it("restores the pre-composition query when the composition is cancelled", () => {
    // Escape hands the document back to what it was, so the settled state the
    // reconcile reads simply carries the earlier query again.
    const gate = gateCompositionStart(OPEN)
    const result = gateCompositionEnd(gate, renderState("美"))
    expect(result.call).toEqual({ type: "update", state: renderState("美") })
  })
})

import { Extension } from "@tiptap/core"
import { Plugin, PluginKey } from "@tiptap/pm/state"
import Suggestion, {
  SuggestionPluginKey,
  type SuggestionProps,
} from "@tiptap/suggestion"

import {
  gateCompositionEnd,
  gateCompositionSettled,
  gateCompositionStart,
  gateSuggestionEvent,
  shouldAllowMention,
  INITIAL_MENTION_GATE,
  type MentionGateCall,
  type MentionGateEvent,
} from "./ime-suggestion-gate"
import { findMentionMatch } from "./mention-match"

/** Live render state the plugin pushes to React while the `@` panel is open. */
export interface MentionRenderState {
  query: string
  /** Document range covering `@` + query, replaced when a row is chosen. */
  range: { from: number; to: number }
  /**
   * Live caret-rect getter (viewport coords), or null if unknown. Call it at
   * position time — not once at trigger time — so the popup re-anchors to the
   * current caret after a window resize, editor scroll, or page scroll while it
   * is open.
   */
  getClientRect: (() => DOMRect | null) | null
}

/**
 * Callbacks the React layer supplies so the suggestion plugin can drive a React
 * popup that lives in the editor's component tree (where data hooks work). The
 * plugin owns trigger detection; React owns data + rendering + insertion.
 */
export interface MentionController {
  onStart: (state: MentionRenderState) => void
  onUpdate: (state: MentionRenderState) => void
  onExit: () => void
  /** Forwarded keydown; return true if the popup consumed it. */
  onKeyDown: (event: KeyboardEvent) => boolean
}

export interface MentionSuggestionOptions {
  controller: MentionController
}

const NOOP_CONTROLLER: MentionController = {
  onStart: () => {},
  onUpdate: () => {},
  onExit: () => {},
  onKeyDown: () => false,
}

/** Identifies the companion plugin that watches for IME compositions. */
const compositionPluginKey = new PluginKey("mentionSuggestionComposition")

/**
 * The slice of `@tiptap/suggestion`'s (untyped) plugin state the composition
 * reconcile reads back. `query`/`range` are only meaningful while `active`.
 */
interface SuggestionPluginState {
  active: boolean
  query: string | null
  range: { from: number; to: number }
}

function toRenderState(props: SuggestionProps): MentionRenderState {
  return {
    query: props.query,
    range: props.range,
    // Keep the getter itself (not a snapshot) so reposition reads live coords.
    getClientRect: props.clientRect ?? null,
  }
}

/**
 * Tiptap extension wiring `@tiptap/suggestion` (trigger `@`) to a
 * {@link MentionController}. Data fetching, rendering and insertion are handled
 * by the controller's React popup, so the plugin's own `items`/`command` are
 * intentionally inert.
 *
 * A second plugin rides along to keep the panel intact across an IME
 * composition — see `./ime-suggestion-gate` for what the composition does to the
 * suggestion state machine and why the panel is frozen rather than followed.
 */
export const MentionSuggestion = Extension.create<MentionSuggestionOptions>({
  name: "mentionSuggestion",

  addOptions() {
    return { controller: NOOP_CONTROLLER }
  },

  addProseMirrorPlugins() {
    const controller = this.options.controller
    const editor = this.editor

    // Per-editor composition gate. Mutable because ProseMirror delivers events
    // one at a time; every decision it drives is a pure function of this value.
    let gate = INITIAL_MENTION_GATE
    // The last state the plugin produced, frozen ones included. Its
    // `getClientRect` is a live closure (the plugin resolves the decoration node
    // on each call), so the reconcile can hand the same getter back instead of
    // inventing one for the state object it assembles itself.
    let last: MentionRenderState | null = null
    let resyncTimer: ReturnType<typeof setTimeout> | null = null

    const emit = (call: MentionGateCall | null) => {
      if (!call) return
      if (call.type === "exit") controller.onExit()
      else if (call.type === "start") controller.onStart(call.state)
      else controller.onUpdate(call.state)
    }

    const receive = (event: MentionGateEvent) => {
      if (event.type !== "exit") last = event.state
      const next = gateSuggestionEvent(gate, event)
      gate = next.gate
      emit(next.call)
    }

    const resync = () => {
      resyncTimer = null
      if (editor.isDestroyed) return
      const view = editor.view
      // The next composition already started (committing one character straight
      // into the next): stay frozen — its own end schedules the next reconcile.
      if (view.composing) return
      // Order matters: the freeze has to be off before the transaction below,
      // because `allow` reads it and would otherwise refuse to revive a
      // suggestion the composition knocked out.
      gate = gateCompositionSettled(gate)
      // Make the suggestion plugin re-match the settled document. The composed
      // text usually lands *during* the composition, so this empty transaction
      // is often the only one that ever runs the matcher with the composition
      // over. It changes no content, so no `onUpdate` (and no draft save) fires.
      view.dispatch(view.state.tr)
      const pluginState = SuggestionPluginKey.getState(view.state) as
        | SuggestionPluginState
        | undefined
      const live: MentionRenderState | null =
        pluginState?.active === true
          ? {
              query: pluginState.query ?? "",
              range: pluginState.range,
              getClientRect: last?.getClientRect ?? null,
            }
          : null
      const next = gateCompositionEnd(gate, live)
      gate = next.gate
      emit(next.call)
    }

    const scheduleResync = () => {
      if (resyncTimer !== null) clearTimeout(resyncTimer)
      // A `handleDOMEvents` prop runs BEFORE ProseMirror's own handler for the
      // same event, so right now `view.composing` is still true and the composed
      // text has not been read out of the DOM yet (ProseMirror flushes it in a
      // microtask). Waiting one task puts the reconcile after both.
      resyncTimer = setTimeout(resync, 0)
    }

    return [
      Suggestion({
        editor,
        char: "@",
        allowSpaces: false,
        findSuggestionMatch: findMentionMatch,
        items: () => [],
        command: () => {},
        allow: ({ state, isActive }) =>
          shouldAllowMention({
            // The gate's flag is the wider window of the two: `compositionstart`
            // reaches us before ProseMirror sets `view.composing`, and the
            // reconcile lifts the freeze a task after `compositionend`.
            composing: gate.composing || editor.view.composing,
            isActive: isActive === true,
            inCodeBlock: state.selection.$from.parent.type.spec.code === true,
          }),
        render: () => ({
          onStart: (props) =>
            receive({ type: "start", state: toRenderState(props) }),
          onUpdate: (props) =>
            receive({ type: "update", state: toRenderState(props) }),
          onExit: () => receive({ type: "exit" }),
          onKeyDown: (props) => controller.onKeyDown(props.event),
        }),
      }),
      new Plugin({
        key: compositionPluginKey,
        props: {
          handleDOMEvents: {
            // Both handlers must report the event as unhandled: ProseMirror
            // skips its own composition handling for an event a plugin claims,
            // which would break IME input outright.
            compositionstart: () => {
              gate = gateCompositionStart(gate)
              return false
            },
            compositionend: () => {
              scheduleResync()
              return false
            },
          },
        },
        view: () => ({
          destroy: () => {
            if (resyncTimer !== null) clearTimeout(resyncTimer)
          },
        }),
      }),
    ]
  },
})

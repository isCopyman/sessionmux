import { useCallback, useEffect, useState } from "react"

import {
  nextKeySelection,
  type SessionClickIntent,
} from "@/lib/session-multi-select"
import {
  itemKeyOf,
  type SidebarSelectionItem,
} from "@/lib/sidebar-item-selection"

/**
 * Mixed Session/Room multi-select for the sidebar lists. Same gesture
 * semantics as `useSessionMultiSelect` (open / ctrl-toggle / shift-range,
 * Escape clears), but the selection is keyed by the prefixed item keys from
 * `sidebar-item-selection`, so Rooms join the same selection as Sessions.
 */
export function useSidebarMultiSelect() {
  const [state, setState] = useState<{
    selected: Map<string, SidebarSelectionItem>
    anchorKey: string | null
  }>(() => ({ selected: new Map(), anchorKey: null }))

  const clear = useCallback(() => {
    setState({ selected: new Map(), anchorKey: null })
  }, [])

  const apply = useCallback(
    (
      clicked: SidebarSelectionItem,
      intent: SessionClickIntent,
      orderedKeys: readonly string[],
      lookup: (key: string) => SidebarSelectionItem | undefined
    ) => {
      setState((current) =>
        nextKeySelection({
          current: current.selected,
          orderedKeys,
          lookup,
          clickedKey: itemKeyOf(clicked),
          clicked,
          intent,
          anchorKey: current.anchorKey,
        })
      )
    },
    []
  )

  useEffect(() => {
    if (state.selected.size === 0) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return
      if (
        document.querySelector(
          '[role="dialog"], [role="alertdialog"], [role="menu"]'
        )
      ) {
        return
      }
      event.preventDefault()
      clear()
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [clear, state.selected.size])

  return {
    selected: state.selected,
    selectedCount: state.selected.size,
    apply,
    clear,
  }
}

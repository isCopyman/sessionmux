import { useCallback, useEffect, useState } from "react"

import {
  nextSessionSelection,
  type SessionClickIntent,
} from "@/lib/session-multi-select"

export function useSessionMultiSelect<T extends { id: number }>() {
  const [state, setState] = useState<{
    selected: Map<number, T>
    anchorId: number | null
  }>(() => ({ selected: new Map(), anchorId: null }))

  const clear = useCallback(() => {
    setState({ selected: new Map(), anchorId: null })
  }, [])

  const apply = useCallback(
    (
      clicked: T,
      intent: SessionClickIntent,
      orderedIds: readonly number[],
      lookup: (id: number) => T | undefined
    ) => {
      setState((current) =>
        nextSessionSelection(
          current.selected,
          orderedIds,
          lookup,
          clicked,
          intent,
          current.anchorId
        )
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

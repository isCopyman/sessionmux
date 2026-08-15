import { fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { useLongPressDrag } from "./use-long-press-drag"

vi.mock("motion/react", () => ({
  useDragControls: () => ({ start: vi.fn() }),
}))

function Harness({
  onClick,
  onDragSettle,
}: {
  onClick: () => void
  onDragSettle: () => void
}) {
  const { gestureHandlers } = useLongPressDrag({
    enabled: false,
    onStart: () => {},
    onEnd: () => {},
    dragSettleMs: 200,
    onDragSettle,
  })
  return (
    <button data-testid="tab" {...gestureHandlers} onClick={onClick}>
      Session
    </button>
  )
}

describe("useLongPressDrag click recovery", () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(1_000)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it("swallows the immediate post-drag click, then recovers without waiting for the settle timer", () => {
    const onClick = vi.fn()
    const onDragSettle = vi.fn()
    render(<Harness onClick={onClick} onDragSettle={onDragSettle} />)
    const tab = screen.getByTestId("tab")

    fireEvent.dragStart(tab)
    fireEvent.dragEnd(tab)
    fireEvent.click(tab)
    expect(onClick).not.toHaveBeenCalled()

    // Advance the wall clock without executing pending timers. Click recovery
    // must be timestamp-based; reconciliation is free to cancel visual cleanup.
    vi.setSystemTime(1_201)
    fireEvent.click(tab)
    expect(onClick).toHaveBeenCalledTimes(1)
    expect(onDragSettle).not.toHaveBeenCalled()
  })
})

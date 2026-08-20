import { act, render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"

import { NativeContextMenuGuard } from "./native-context-menu-guard"

function rightClick(target: Element, init: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    button: 2,
    ...init,
  })
  act(() => {
    target.dispatchEvent(event)
  })
  return event
}

function prevented(testId: string, init?: MouseEventInit): boolean {
  return rightClick(screen.getByTestId(testId), init).defaultPrevented
}

describe("NativeContextMenuGuard", () => {
  it("cancels the native menu on ordinary content", () => {
    render(
      <>
        <NativeContextMenuGuard />
        <div data-testid="row">session</div>
      </>
    )

    expect(prevented("row")).toBe(true)
  })

  it("leaves editable targets to the native menu", () => {
    render(
      <>
        <NativeContextMenuGuard />
        <input data-testid="input" readOnly value="" />
        <textarea data-testid="textarea" readOnly value="" />
        <div contentEditable suppressContentEditableWarning data-testid="rich">
          <span data-testid="rich-child">typed</span>
        </div>
      </>
    )

    expect(prevented("input")).toBe(false)
    expect(prevented("textarea")).toBe(false)
    // Ancestor walk: the press lands on a child of the editable host.
    expect(prevented("rich-child")).toBe(false)
  })

  it("honours the data-native-context-menu opt-out and its subtree", () => {
    render(
      <>
        <NativeContextMenuGuard />
        <div data-native-context-menu data-testid="host">
          <span data-testid="host-child">child</span>
        </div>
      </>
    )

    expect(prevented("host")).toBe(false)
    expect(prevented("host-child")).toBe(false)
  })

  it("leaves an existing custom context menu working", () => {
    render(
      <>
        <NativeContextMenuGuard />
        <ContextMenu>
          <ContextMenuTrigger>
            <span data-testid="trigger">conversation</span>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem>Rename</ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      </>
    )

    // Radix mounts the content lazily, so the item is absent while closed.
    expect(screen.queryByText("Rename")).not.toBeInTheDocument()

    rightClick(screen.getByTestId("trigger"), { clientX: 10, clientY: 10 })

    // Radix composes its opener with `checkForDefaultPrevented`, so it refuses
    // to open on an event that already carries `defaultPrevented`. This failing
    // would mean the guard had moved ahead of the trigger (capture phase).
    expect(screen.getByText("Rename")).toBeInTheDocument()
  })

  it("passes Shift+right-click through outside production builds", () => {
    // Vitest runs with NODE_ENV=test, so the non-production branch is live in
    // this suite. `next build` inlines "production" and drops it from the
    // static export; `next dev` keeps it, which is where the hatch is for.
    expect(process.env.NODE_ENV).not.toBe("production")

    render(
      <>
        <NativeContextMenuGuard />
        <div data-testid="row">session</div>
      </>
    )

    expect(prevented("row", { shiftKey: true })).toBe(false)
    expect(prevented("row")).toBe(true)
  })

  it("unhooks the listener on unmount", () => {
    // Owned by the test, not by the render container, so it outlives unmount
    // and can still bubble to `document`.
    const row = document.createElement("div")
    document.body.append(row)
    const { unmount } = render(<NativeContextMenuGuard />)

    expect(rightClick(row).defaultPrevented).toBe(true)

    unmount()
    expect(rightClick(row).defaultPrevented).toBe(false)

    row.remove()
  })
})

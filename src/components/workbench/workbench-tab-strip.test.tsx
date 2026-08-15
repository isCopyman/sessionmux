import { act, fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"
import enMessages from "@/i18n/messages/en.json"
import { WorkbenchTabStrip } from "./workbench-tab-strip"

const mocks = vi.hoisted(() => ({
  hydrate: vi.fn(async () => {}),
  createAndSwitch: vi.fn(async () => ({})),
  duplicateAndSwitch: vi.fn(async () => ({})),
  previewOrder: vi.fn(),
  persistOrder: vi.fn(async () => {}),
  switchWorkbench: vi.fn(async () => {}),
  items: [
    {
      id: 1,
      name: "Main",
      position: 0,
      created_at: "2026-08-15T00:00:00Z",
      updated_at: "2026-08-15T00:00:00Z",
    },
    {
      id: 2,
      name: "Review",
      position: 1,
      created_at: "2026-08-15T00:00:00Z",
      updated_at: "2026-08-15T00:00:00Z",
    },
  ],
  activeWorkbenchId: 1,
  switchingWorkbenchId: null as number | null,
}))

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: mocks.items,
      hydrated: true,
      loading: false,
      hydrate: mocks.hydrate,
      createAndSwitch: mocks.createAndSwitch,
      duplicateAndSwitch: mocks.duplicateAndSwitch,
      previewOrder: mocks.previewOrder,
      persistOrder: mocks.persistOrder,
    }),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabStore: (selector: (state: unknown) => unknown) =>
    selector({
      activeWorkbenchId: mocks.activeWorkbenchId,
      switchingWorkbenchId: mocks.switchingWorkbenchId,
      switchWorkbench: mocks.switchWorkbench,
    }),
}))

function renderStrip() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <WorkbenchTabStrip leftInset={72} rightInset={96} />
    </NextIntlClientProvider>
  )
}

describe("WorkbenchTabStrip", () => {
  beforeEach(() => {
    mocks.hydrate.mockClear()
    mocks.createAndSwitch.mockClear()
    mocks.duplicateAndSwitch.mockClear()
    mocks.previewOrder.mockClear()
    mocks.persistOrder.mockClear()
    mocks.switchWorkbench.mockClear()
    mocks.activeWorkbenchId = 1
    mocks.switchingWorkbenchId = null
  })

  it("renders saved workbenches as window-level tabs and switches them", () => {
    renderStrip()

    const main = screen.getByRole("tab", { name: "Main" })
    const review = screen.getByRole("tab", { name: "Review" })
    expect(main.getAttribute("aria-selected")).toBe("true")
    expect(review.getAttribute("aria-selected")).toBe("false")

    fireEvent.click(review)
    expect(mocks.switchWorkbench).toHaveBeenCalledWith(2)
  })

  it("creates a named workbench from the trailing plus button", async () => {
    renderStrip()
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "New workbench" }))
    })
    expect(mocks.createAndSwitch).toHaveBeenCalledWith("Workbench 3")
  })

  it("shows which target is restoring and locks competing switches", () => {
    mocks.switchingWorkbenchId = 2
    renderStrip()
    expect(screen.getByRole("tab", { name: "Review" })).toBeDisabled()
    expect(screen.getByRole("tab", { name: "Main" })).toBeDisabled()
  })

  it("duplicates a workbench from its context menu", async () => {
    renderStrip()
    fireEvent.contextMenu(screen.getByRole("tab", { name: "Review" }))
    await act(async () => {
      fireEvent.click(
        screen.getByRole("menuitem", { name: "Duplicate workbench" })
      )
    })
    expect(mocks.duplicateAndSwitch).toHaveBeenCalledWith(2, "Review Copy")
  })

  it("supports keyboard reordering with Alt+Arrow", async () => {
    renderStrip()
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("tab", { name: "Main" }), {
        key: "ArrowRight",
        altKey: true,
      })
    })
    expect(mocks.previewOrder).toHaveBeenCalledWith([
      mocks.items[1],
      mocks.items[0],
    ])
    expect(mocks.persistOrder).toHaveBeenCalledTimes(1)
  })
})

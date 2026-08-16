import { act, render, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { OrganizationSyncListener } from "./organization-sync-listener"

const h = vi.hoisted(() => ({
  subscribe: vi.fn(),
  onReconnect: vi.fn(),
  collectionHydrate: vi.fn(),
  workbenchHydrate: vi.fn(),
  invalidate: vi.fn(),
  dispose: vi.fn(),
  offReconnect: vi.fn(),
  handler: undefined as ((change: unknown) => void) | undefined,
  reconnect: undefined as (() => void) | undefined,
}))

vi.mock("@/lib/platform", () => ({
  subscribe: (...args: unknown[]) => h.subscribe(...args),
  onTransportReconnect: (...args: unknown[]) => h.onReconnect(...args),
}))

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: {
    getState: () => ({ hydrate: h.collectionHydrate }),
  },
}))

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: {
    getState: () => ({ hydrate: h.workbenchHydrate }),
  },
}))

vi.mock("@/stores/organization-revision-store", () => ({
  useOrganizationRevisionStore: {
    getState: () => ({ invalidate: h.invalidate }),
  },
}))

describe("OrganizationSyncListener", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.handler = undefined
    h.reconnect = undefined
    h.collectionHydrate.mockResolvedValue(undefined)
    h.workbenchHydrate.mockResolvedValue(undefined)
    h.subscribe.mockImplementation(
      (_event: string, handler: (change: unknown) => void) => {
        h.handler = handler
        return Promise.resolve(h.dispose)
      }
    )
    h.onReconnect.mockImplementation((callback: () => void) => {
      h.reconnect = callback
      return h.offReconnect
    })
  })

  it("refetches only affected backend facts without navigation", async () => {
    render(<OrganizationSyncListener />)
    await waitFor(() => expect(h.handler).toBeTypeOf("function"))

    act(() => {
      h.handler?.({ entity: "workbench", id: 7, origin: "host-control" })
    })
    expect(h.invalidate).toHaveBeenCalledTimes(1)
    expect(h.workbenchHydrate).toHaveBeenCalledTimes(1)
    expect(h.collectionHydrate).not.toHaveBeenCalled()

    act(() => {
      h.handler?.({
        entity: "collection_membership",
        id: 3,
        origin: "host-control",
      })
    })
    expect(h.invalidate).toHaveBeenCalledTimes(2)
    expect(h.collectionHydrate).toHaveBeenCalledWith(true)
    expect(h.workbenchHydrate).toHaveBeenCalledTimes(1)
  })

  it("reconciles both stores after reconnect and cleans up listeners", async () => {
    const view = render(<OrganizationSyncListener />)
    await waitFor(() => expect(h.reconnect).toBeTypeOf("function"))

    act(() => h.reconnect?.())
    expect(h.collectionHydrate).toHaveBeenCalledWith(true)
    expect(h.workbenchHydrate).toHaveBeenCalledTimes(1)
    expect(h.invalidate).toHaveBeenCalledTimes(1)

    view.unmount()
    await waitFor(() => expect(h.dispose).toHaveBeenCalledTimes(1))
    expect(h.offReconnect).toHaveBeenCalledTimes(1)
  })
})

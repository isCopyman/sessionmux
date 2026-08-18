import { act, fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { forwardRef, useImperativeHandle } from "react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { Sidebar } from "./sidebar"
import enMessages from "@/i18n/messages/en.json"

// Stable spies + mutable active-folder, referenced from the hoisted mock
// factories below (vi.mock is hoisted above imports).
const spies = vi.hoisted(() => ({
  openNewConversationTab: vi.fn(),
  openChatModeTab: vi.fn(),
  openTab: vi.fn(),
  setSearchOpen: vi.fn(),
  setRoute: vi.fn(),
  openConversations: vi.fn(),
  // Latest props the (stubbed) conversation list was rendered with, so tests can
  // assert what the sidebar threads down (e.g. showWorktrees / showCompleted).
  listProps: null as {
    showWorktrees?: boolean
    showCompleted?: boolean
    showRecent?: boolean
    sectionOrder?: readonly string[]
    unreadByConversation?: ReadonlyMap<number, number>
  } | null,
  sessionCenterOpen: false,
  sessionCenterCollection: null as number | "unclassified" | null,
  collectionShowsSessions: false,
  workbenchUnread: null as ReadonlyMap<number, number> | null,
  collectionUnread: null as ReadonlyMap<number, number> | null,
  collectionScrollToActive: vi.fn(),
}))
const mockState = vi.hoisted(() => ({
  activeFolder: { id: 7, path: "/x" } as { id: number; path: string } | null,
  allFolders: [{ id: 7, path: "/x" }],
}))

// The conversation list is irrelevant here — stub it so the test exercises only
// the sidebar's header + fixed New chat / Search region.
vi.mock("@/components/conversations/sidebar-conversation-list", () => ({
  SidebarConversationList: (props: {
    showWorktrees?: boolean
    showCompleted?: boolean
    showRecent?: boolean
    sectionOrder?: readonly string[]
    unreadByConversation?: ReadonlyMap<number, number>
  }) => {
    spies.listProps = props
    return null
  },
}))
vi.mock("@/components/workbench/workbench-tree", () => ({
  WorkbenchTree: ({
    unreadByConversation,
  }: {
    unreadByConversation?: ReadonlyMap<number, number>
  }) => {
    spies.workbenchUnread = unreadByConversation ?? null
    return <div>Workbench tree</div>
  },
}))
vi.mock("@/components/collections/collection-tree", () => ({
  CollectionTree: forwardRef(function MockCollectionTree(
    {
      onOpenScope,
      onNewSession,
      showSessions,
      unreadByConversation,
    }: {
      onOpenScope: (scope: number | "unclassified") => void
      onNewSession?: (rootFolderId: number) => void
      showSessions?: boolean
      unreadByConversation?: ReadonlyMap<number, number>
    },
    ref
  ) {
    useImperativeHandle(ref, () => ({
      scrollToActive: spies.collectionScrollToActive,
    }))
    // eslint-disable-next-line react-hooks/immutability -- test probe captures rendered props
    spies.collectionShowsSessions = showSessions === true
    // eslint-disable-next-line react-hooks/immutability -- test probe captures rendered props
    spies.collectionUnread = unreadByConversation ?? null
    return (
      <>
        <button type="button" onClick={() => onOpenScope(42)}>
          Research collection
        </button>
        <button type="button" onClick={() => onNewSession?.(7)}>
          New session at path
        </button>
      </>
    )
  }),
}))
vi.mock("@/components/conversations/conversation-manage-dialog", () => ({
  ConversationManageDialog: ({
    open,
    initialCollection,
  }: {
    open: boolean
    initialCollection?: number | "unclassified" | null
  }) => {
    spies.sessionCenterOpen = open
    spies.sessionCenterCollection = initialCollection ?? null
    return open ? <div>Session Center Dialog</div> : null
  },
}))
vi.mock("@/contexts/sidebar-context", () => ({
  useSidebarContext: () => ({ isOpen: true, toggle: vi.fn() }),
}))
vi.mock("@/contexts/active-folder-context", () => ({
  useActiveFolder: () => ({ activeFolder: mockState.activeFolder }),
}))
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({ allFolders: mockState.allFolders }),
}))
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({
    openNewConversationTab: spies.openNewConversationTab,
    openChatModeTab: spies.openChatModeTab,
    openTab: spies.openTab,
  }),
}))
vi.mock("@/contexts/search-dialog-context", () => ({
  useSearchDialog: () => ({ open: false, setOpen: spies.setSearchOpen }),
}))
vi.mock("@/contexts/automations-view-context", () => ({
  useAutomationsView: () => ({
    automations: [],
    unseenFailures: 0,
    refetch: async () => {},
  }),
}))
vi.mock("@/contexts/tasks-view-context", () => ({
  useTasksView: () => ({
    tasks: [],
    attentionCount: 0,
    refetch: async () => {},
  }),
}))
vi.mock("@/contexts/workbench-route-context", () => ({
  useWorkbenchRoute: () => ({
    routeId: "conversations",
    isConversations: true,
    setRoute: spies.setRoute,
    openConversations: spies.openConversations,
  }),
}))
vi.mock("@/hooks/use-is-mac", () => ({ useIsMac: () => false }))
vi.mock("@/hooks/use-shortcut-settings", () => ({
  useShortcutSettings: () => ({
    shortcuts: { toggle_search: "mod+k", new_conversation: "mod+t" },
  }),
}))
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }))
vi.mock("@/hooks/use-appearance", () => ({
  useZoomLevel: () => ({ zoomLevel: 100, setZoomLevel: () => {} }),
}))
vi.mock("@/hooks/use-collaboration-unread-overview", () => ({
  useCollaborationUnreadOverview: () => ({
    overview: {
      totalUnreadCount: 3,
      sessions: [{ conversationId: 42, unreadCount: 3 }],
    },
    unreadByConversation: new Map([[42, 3]]),
    hydrated: true,
    error: null,
    reload: async () => {},
  }),
}))

function renderSidebar() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <Sidebar />
    </NextIntlClientProvider>
  )
}

describe("Sidebar — fixed New chat / Search region", () => {
  beforeEach(() => {
    localStorage.clear()
    spies.openNewConversationTab.mockClear()
    spies.openChatModeTab.mockClear()
    spies.setSearchOpen.mockClear()
    spies.setRoute.mockClear()
    spies.openConversations.mockClear()
    spies.sessionCenterOpen = false
    spies.sessionCenterCollection = null
    spies.collectionShowsSessions = false
    spies.workbenchUnread = null
    spies.collectionUnread = null
    spies.collectionScrollToActive.mockClear()
    spies.listProps = null
    mockState.activeFolder = { id: 7, path: "/x" }
  })

  it("Automations navigates to the automations route", () => {
    const { getByText } = renderSidebar()
    fireEvent.click(getByText("Automations"))
    expect(spies.setRoute).toHaveBeenCalledWith("automations")
  })

  it("Rooms navigates to the rooms route", () => {
    const { getByText } = renderSidebar()
    fireEvent.click(getByText("Rooms"))
    expect(spies.setRoute).toHaveBeenCalledWith("rooms")
  })

  it("New chat returns to the conversation workspace", () => {
    const { getByText } = renderSidebar()
    fireEvent.click(getByText("New chat"))
    expect(spies.openConversations).toHaveBeenCalled()
  })

  it("New chat opens a conversation tab in the active folder", () => {
    const { getByText } = renderSidebar()
    fireEvent.click(getByText("New chat"))
    expect(spies.openNewConversationTab).toHaveBeenCalledWith(7, "/x")
  })

  it("Search opens the shared search dialog", () => {
    const { getByText } = renderSidebar()
    fireEvent.click(getByText("Search"))
    expect(spies.setSearchOpen).toHaveBeenCalledWith(true)
  })

  it("Session Center opens the global conversation manager", () => {
    const { getByText } = renderSidebar()
    expect(screen.queryByLabelText("3 unread")).toBeNull()
    fireEvent.click(getByText("Session Center"))
    expect(spies.sessionCenterOpen).toBe(true)
    expect(getByText("Session Center Dialog")).toBeTruthy()
  })

  it("opens Session Center scoped from the Collection tree", () => {
    const { getByText } = renderSidebar()
    fireEvent.click(getByText("Research collection"))
    expect(spies.sessionCenterOpen).toBe(true)
    expect(spies.sessionCenterCollection).toBe(42)
  })

  it("renders New chat and Search shortcut hints", () => {
    const { getByText } = renderSidebar()
    // isMac=false → "mod" formats as "Ctrl". The badges are opacity-0 until the
    // row is hovered/focused but stay in the DOM, so getByText resolves them.
    expect(getByText("Ctrl+T")).toBeTruthy()
    expect(getByText("Ctrl+K")).toBeTruthy()
  })

  it("defaults to Workbench + path-rooted Collection organization", () => {
    renderSidebar()
    expect(screen.getByText("Workbench tree")).toBeTruthy()
    expect(spies.collectionShowsSessions).toBe(true)
    expect(spies.listProps).toBeNull()
  })

  it("locates the active Session inside the Collection tree", () => {
    renderSidebar()
    fireEvent.click(
      screen.getByRole("button", { name: "Locate Active Conversation" })
    )
    expect(spies.collectionScrollToActive).toHaveBeenCalledOnce()
  })

  it("creates a Session at the Collection root's execution path", async () => {
    const user = userEvent.setup()
    renderSidebar()

    expect(spies.collectionShowsSessions).toBe(true)
    await user.click(
      screen.getByRole("button", { name: "New session at path" })
    )
    expect(spies.openNewConversationTab).toHaveBeenCalledWith(7, "/x")
  })

  it("exposes Collection as a one-click header toggle", async () => {
    const user = userEvent.setup()
    renderSidebar()

    const toggle = screen.getByRole("button", { name: "By run location" })
    expect(toggle).toHaveAttribute("aria-pressed", "true")
    await user.click(toggle)
    expect(
      screen.getByRole("button", { name: "By Collection" })
    ).toHaveAttribute("aria-pressed", "false")
    expect(localStorage.getItem("workspace:sidebar-organization-mode")).toBe(
      "locations"
    )

    await user.click(screen.getByRole("button", { name: "View options" }))
    expect(
      screen.queryByRole("menuitemradio", { name: "By run location" })
    ).toBeNull()
  })

  it("falls back to chat mode (never disabled) when no folder is active", () => {
    mockState.activeFolder = null
    const { getByText } = renderSidebar()
    const btn = getByText("New chat").closest("button") as HTMLButtonElement
    // Defense-in-depth: the button stays clickable so a workspace that recovered
    // to no active folder is never a dead end — it opens folderless chat mode.
    expect(btn.disabled).toBe(false)
    fireEvent.click(btn)
    expect(spies.openChatModeTab).toHaveBeenCalled()
    expect(spies.openNewConversationTab).not.toHaveBeenCalled()
  })
})

describe("Sidebar — Show worktree folders toggle", () => {
  beforeEach(() => {
    localStorage.clear()
    localStorage.setItem("workspace:sidebar-organization-mode", "locations")
    spies.listProps = null
    mockState.activeFolder = { id: 7, path: "/x" }
  })

  it("defaults Show worktree folders on and threads it to the conversation list", () => {
    renderSidebar()
    expect(spies.listProps?.showWorktrees).toBe(true)
  })

  it("respects an explicitly-stored 'false' from localStorage", () => {
    localStorage.setItem("workspace:sidebar-show-worktrees", "false")
    renderSidebar()
    // Hydration runs in a mount effect (flushed by render's act): a user who
    // unchecked it keeps it off despite the default-on.
    expect(spies.listProps?.showWorktrees).toBe(false)
  })

  it("toggling the funnel item off persists the choice and threads it down", async () => {
    const user = userEvent.setup()
    renderSidebar()
    // Default on with a cleared store.
    expect(spies.listProps?.showWorktrees).toBe(true)

    await user.click(screen.getByRole("button", { name: "View options" }))
    await user.click(
      screen.getByRole("menuitemcheckbox", { name: "Show worktree folders" })
    )

    expect(localStorage.getItem("workspace:sidebar-show-worktrees")).toBe(
      "false"
    )
    expect(spies.listProps?.showWorktrees).toBe(false)
  })
})

describe("Sidebar — Show completed default", () => {
  beforeEach(() => {
    localStorage.clear()
    localStorage.setItem("workspace:sidebar-organization-mode", "locations")
    spies.listProps = null
    mockState.activeFolder = { id: 7, path: "/x" }
  })

  it("defaults Show completed off and threads it to the conversation list", () => {
    renderSidebar()
    expect(spies.listProps?.showCompleted).toBe(false)
  })

  it("respects an explicitly-stored 'true' from localStorage", () => {
    localStorage.setItem("workspace:sidebar-show-completed", "true")
    renderSidebar()
    expect(spies.listProps?.showCompleted).toBe(true)
  })
})

describe("Sidebar — Show Recent group toggle", () => {
  beforeEach(() => {
    localStorage.clear()
    localStorage.setItem("workspace:sidebar-organization-mode", "locations")
    spies.listProps = null
    mockState.activeFolder = { id: 7, path: "/x" }
  })

  it("defaults Show Recent on and threads it to the conversation list", () => {
    renderSidebar()
    expect(spies.listProps?.showRecent).toBe(true)
  })

  it("respects an explicitly-stored 'false' from localStorage", () => {
    localStorage.setItem("workspace:sidebar-show-recent", "false")
    renderSidebar()
    expect(spies.listProps?.showRecent).toBe(false)
  })

  it("toggling the funnel item off persists the choice and keeps the menu open", async () => {
    const user = userEvent.setup()
    renderSidebar()

    await user.click(screen.getByRole("button", { name: "View options" }))
    await user.click(
      screen.getByRole("menuitemcheckbox", { name: "Show Recent group" })
    )

    expect(localStorage.getItem("workspace:sidebar-show-recent")).toBe("false")
    expect(spies.listProps?.showRecent).toBe(false)
    // The view-options menu is a settings panel: flipping one option must not
    // dismiss it, or changing two costs two trips through the trigger.
    expect(
      screen.getByRole("menuitemcheckbox", { name: "Show Recent group" })
    ).toBeTruthy()
  })
})

describe("Sidebar — Section order control", () => {
  beforeEach(() => {
    localStorage.clear()
    localStorage.setItem("workspace:sidebar-organization-mode", "locations")
    spies.listProps = null
    mockState.activeFolder = { id: 7, path: "/x" }
  })

  // The order rows are `menuitem`s labelled "<name> — position N of 3"; the
  // move buttons inside them are labelled "<name> — Move up/down".
  const orderRowNames = () =>
    screen
      .getAllByRole("menuitem")
      .map((el) => el.getAttribute("aria-label") ?? "")
      .filter((label) => label.includes("position"))

  it("defaults to Folders → Chat → Recent", async () => {
    const user = userEvent.setup()
    renderSidebar()
    await user.click(screen.getByRole("button", { name: "View options" }))

    expect(orderRowNames()).toEqual([
      "Folders — position 1 of 3",
      "Chat — position 2 of 3",
      "Recent — position 3 of 3",
    ])
    expect(spies.listProps?.sectionOrder).toEqual([
      "folders",
      "chats",
      "recent",
    ])
  })

  it("moves a section up, persists the new order and threads it down", async () => {
    const user = userEvent.setup()
    renderSidebar()
    await user.click(screen.getByRole("button", { name: "View options" }))
    await user.click(screen.getByRole("button", { name: "Recent — Move up" }))

    expect(orderRowNames()).toEqual([
      "Folders — position 1 of 3",
      "Recent — position 2 of 3",
      "Chat — position 3 of 3",
    ])
    expect(spies.listProps?.sectionOrder).toEqual([
      "folders",
      "recent",
      "chats",
    ])
    expect(
      JSON.parse(localStorage.getItem("workspace:sidebar-section-order") ?? "")
    ).toEqual(["folders", "recent", "chats"])
  })

  it("disables the move buttons that would fall off an end", async () => {
    const user = userEvent.setup()
    renderSidebar()
    await user.click(screen.getByRole("button", { name: "View options" }))

    expect(
      screen.getByRole("button", { name: "Folders — Move up" })
    ).toBeDisabled()
    expect(
      screen.getByRole("button", { name: "Recent — Move down" })
    ).toBeDisabled()
    expect(
      screen.getByRole("button", { name: "Folders — Move down" })
    ).not.toBeDisabled()
  })

  it("reorders from the keyboard with Alt+Arrow on the focused row", async () => {
    const user = userEvent.setup()
    renderSidebar()
    await user.click(screen.getByRole("button", { name: "View options" }))

    // The nested move buttons are unreachable by Tab (Radix's menu swallows it)
    // and by the roving focus, so Alt+Arrow on the row is the ONLY keyboard
    // path — if this regresses, the control becomes mouse-only.
    const foldersRow = screen.getByRole("menuitem", {
      name: "Folders — position 1 of 3",
    })
    // Focusing a menu item updates Radix's roving-focus state, so it has to run
    // inside act().
    act(() => foldersRow.focus())
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}")

    expect(orderRowNames()).toEqual([
      "Chat — position 1 of 3",
      "Folders — position 2 of 3",
      "Recent — position 3 of 3",
    ])
    // Focus follows the row it moved, so a second press keeps going.
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "Folders — position 2 of 3"
    )
  })

  it("restores a legacy 'chats-first' preference from an older build", async () => {
    const user = userEvent.setup()
    localStorage.setItem("workspace:sidebar-section-order", "chats-first")
    renderSidebar()
    await user.click(screen.getByRole("button", { name: "View options" }))

    expect(orderRowNames()).toEqual([
      "Chat — position 1 of 3",
      "Folders — position 2 of 3",
      "Recent — position 3 of 3",
    ])
  })

  it("keeps a hidden Recent section listed and reorderable", async () => {
    const user = userEvent.setup()
    localStorage.setItem("workspace:sidebar-show-recent", "false")
    renderSidebar()
    await user.click(screen.getByRole("button", { name: "View options" }))

    // Hiding is a separate preference from position: the row stays so the user
    // can park it where it will reappear.
    await user.click(screen.getByRole("button", { name: "Recent — Move up" }))
    expect(spies.listProps?.sectionOrder).toEqual([
      "folders",
      "recent",
      "chats",
    ])
    expect(spies.listProps?.showRecent).toBe(false)
  })
})

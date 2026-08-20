import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { RoomWorkspace } from "./rooms-page"
import enMessages from "@/i18n/messages/en.json"
import type { RichComposerHandle } from "@/components/chat/composer/rich-composer"
import type {
  CollaborationRoomDetail,
  RoomTimeline,
  RoomTimelineEvent,
} from "@/lib/types"

// RoomWorkspace keeps its RichComposer handle internally; capture it through a
// partial mock that still renders the real composer, so tests can type into
// the very Tiptap editor the send path serializes (mirrors message-input.test).
const composerHandle = vi.hoisted(() => ({
  current: null as RichComposerHandle | null,
}))
vi.mock("@/components/chat/composer/rich-composer", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/chat/composer/rich-composer")
    >()
  const React = await import("react")
  const Captured = React.forwardRef<
    RichComposerHandle,
    React.ComponentProps<typeof actual.RichComposer>
  >((props, ref) => {
    const assign = (handle: RichComposerHandle | null) => {
      composerHandle.current = handle
      if (typeof ref === "function") ref(handle)
      else if (ref) ref.current = handle
    }
    return React.createElement(actual.RichComposer, { ...props, ref: assign })
  })
  Captured.displayName = "CapturedRichComposer"
  return { ...actual, RichComposer: Captured }
})

// virtua renders 0 rows under jsdom. Default: paint every child so existing
// Room tests keep seeing the whole timeline. Off-screen find tests switch
// `visibleAll` off and let scrollToIndex mount the target virtua index.
const virtuaCtl = vi.hoisted(() => ({
  visibleAll: true,
  visible: new Set<number>(),
  scrollToIndex: vi.fn(),
  shift: false,
  onScroll: null as ((offset: number) => void) | null,
}))

vi.mock("virtua", async () => {
  const React = await import("react")
  const MockVirtualizer = React.forwardRef(function MockVirtualizer(
    {
      children,
      shift,
      onScroll,
    }: {
      children?: React.ReactNode
      shift?: boolean
      onScroll?: (offset: number) => void
    },
    ref: React.Ref<unknown>
  ) {
    const [mounted, setMounted] = React.useState<number[]>([])
    React.useEffect(() => {
      virtuaCtl.shift = Boolean(shift)
      virtuaCtl.onScroll = onScroll ?? null
    })
    React.useImperativeHandle(ref, () => ({
      cache: { measured: true },
      scrollOffset: 720,
      scrollSize: 1600,
      viewportSize: 600,
      findItemIndex: () => 0,
      getItemOffset: () => 0,
      getItemSize: () => 0,
      scrollToIndex: (index: number, opts?: unknown) => {
        virtuaCtl.scrollToIndex(index, opts)
        setMounted((prev) => (prev.includes(index) ? prev : [...prev, index]))
      },
      scrollTo: vi.fn(),
      scrollBy: vi.fn(),
    }))
    const childArray = React.Children.toArray(children)
    const rendered = virtuaCtl.visibleAll
      ? childArray
      : childArray.filter(
          (_, index) => virtuaCtl.visible.has(index) || mounted.includes(index)
        )
    return React.createElement(React.Fragment, null, rendered)
  })
  MockVirtualizer.displayName = "MockVirtualizer"
  return { Virtualizer: MockVirtualizer }
})

const api = vi.hoisted(() => ({
  getCollaborationRoom: vi.fn(),
  getCollaborationRoomTimeline: vi.fn(),
  markCollaborationRoomSeen: vi.fn(),
  postCollaborationRoomMessage: vi.fn(),
  refreshCatalog: vi.fn(),
  deleteCollaborationRoom: vi.fn(),
  removeCollaborationRoomMember: vi.fn(),
  addCollaborationRoomPath: vi.fn(),
  removeCollaborationRoomPath: vi.fn(),
  closeTab: vi.fn(),
  openTab: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  getCollaborationRoom: api.getCollaborationRoom,
  getCollaborationRoomTimeline: api.getCollaborationRoomTimeline,
  markCollaborationRoomSeen: api.markCollaborationRoomSeen,
  postCollaborationRoomMessage: api.postCollaborationRoomMessage,
  addCollaborationRoomMembers: vi.fn(),
  removeCollaborationRoomMember: api.removeCollaborationRoomMember,
  renameCollaborationRoom: vi.fn(),
  deleteCollaborationRoom: api.deleteCollaborationRoom,
  addCollaborationRoomPath: api.addCollaborationRoomPath,
  removeCollaborationRoomPath: api.removeCollaborationRoomPath,
  // The room's `@` panel (`useReferenceSearch`/`useFileTree`) pulls these in.
  // Every existing fixture leaves `rootFolderId` unset, so `roomFolderPath` is
  // always null and these were never reached before — but the "manage paths"
  // tests below give the room `additionalPaths`, which alone is now enough to
  // make `useFileTree` resolve a root and actually call `listWorkspaceFiles`.
  listWorkspaceFiles: vi.fn().mockResolvedValue([]),
  gitLog: vi.fn().mockResolvedValue({ entries: [], has_upstream: false }),
  listAllConversations: vi.fn().mockResolvedValue([]),
  // The manage-paths dialog now embeds the shared `DirectoryBrowser`, which
  // browses the server's filesystem through these two.
  getHomeDirectory: vi.fn().mockResolvedValue("/home/dev"),
  listDirectoryEntries: vi.fn().mockResolvedValue([
    { name: "notes", path: "/home/dev/notes", hasChildren: false },
    { name: "extra", path: "/home/dev/extra", hasChildren: false },
  ]),
}))

// Room detail refreshes are event-driven (`ROOM_CHANGED_EVENT`); keep the
// handlers so a test can play the backend's broadcast.
const platform = vi.hoisted(() => ({
  roomChanged: new Set<(payload: unknown) => void>(),
}))

vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn(
    async (event: string, handler: (payload: unknown) => void) => {
      if (event !== "room://changed") return () => {}
      platform.roomChanged.add(handler)
      return () => platform.roomChanged.delete(handler)
    }
  ),
  // `useReferenceSearch` (now wired into the room's `@` panel) pulls in
  // `useAcpAgents`, which calls this unconditionally on mount — see
  // automations-page.test.tsx / task-detail-sheet.follow-up.test.tsx for the
  // same mock, required for the same reason.
  onTransportReconnect: vi.fn(() => () => {}),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({
    openTab: api.openTab,
    closeTab: api.closeTab,
    switchWorkbench: vi.fn(),
  }),
  useTabStore: (selector: (s: { activeWorkbenchId: number }) => unknown) =>
    selector({ activeWorkbenchId: 1 }),
}))

vi.mock("@/hooks/use-open-or-focus-session", () => ({
  useOpenOrFocusSession:
    () =>
    (conversation: {
      folder_id: number
      id: number
      agent_type: string
      title?: string | null
    }) => {
      api.openTab(
        conversation.folder_id,
        conversation.id,
        conversation.agent_type,
        true,
        conversation.title || undefined
      )
      return Promise.resolve("opened")
    },
}))

vi.mock("@/stores/tab-store", () => ({
  makeRoomTabId: (id: string) => `room-${id}`,
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (
    selector: (state: {
      conversations: Array<{
        id: number
        folder_id: number
        title: string
        agent_type: string
      }>
      allFolders: Array<{ id: number; path: string }>
    }) => unknown
  ) =>
    selector({
      conversations: [
        {
          id: 101,
          folder_id: 7,
          title: "Planner",
          agent_type: "codex",
        },
        {
          id: 202,
          folder_id: 7,
          title: "Session D",
          agent_type: "claude_code",
        },
      ],
      // Only the fixtures that set `rootFolderId` reach this list; the rest
      // resolve to "no bound folder".
      allFolders: [{ id: 7, path: "/repo/research" }],
    }),
}))

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: (
    selector: (state: { items: { id: number; name: string }[] }) => unknown
  ) => selector({ items: [{ id: 7, name: "Research" }] }),
}))

vi.mock("@/stores/room-catalog-store", () => {
  const useRoomCatalogStore = () => ({ rooms: [], hydrated: true })
  useRoomCatalogStore.getState = () => ({ refresh: api.refreshCatalog })
  return { useRoomCatalogStore }
})

const runtime = vi.hoisted(() => ({
  byConversationId: new Map<number, { liveMessage: { id: string } | null }>(),
}))

vi.mock("@/stores/conversation-runtime-store", () => ({
  useConversationRuntimeStore: (
    selector: (state: {
      byConversationId: Map<number, { liveMessage: { id: string } | null }>
    }) => unknown
  ) => selector({ byConversationId: runtime.byConversationId }),
}))

const roomId = "rm_plan"

function roomDetail(): CollaborationRoomDetail {
  return {
    id: roomId,
    workbenchId: 1,
    title: "Plan room",
    createdByConversationId: 101,
    members: [
      {
        conversationId: 101,
        title: "Planner",
        agentType: "codex",
        role: "member",
        joinedAt: "2026-08-01T00:00:00.000Z",
      },
    ],
    createdAt: "2026-08-01T00:00:00.000Z",
    updatedAt: "2026-08-18T00:00:00.000Z",
  }
}

function event(overrides: Partial<RoomTimelineEvent> = {}): RoomTimelineEvent {
  return {
    id: "evt-new",
    roomId,
    source: {
      conversationId: 101,
      title: "Planner",
      agentType: "codex",
      folderPath: "/repo",
      backend: "current",
    },
    subject: "newest",
    body: "newest post",
    expectsReply: false,
    urgency: "normal",
    mentionConversationIds: [],
    mentionHuman: false,
    authorKind: "session",
    createdAt: "2026-08-18T12:00:00.000Z",
    ...overrides,
  }
}

function timeline(
  events: RoomTimelineEvent[],
  truncated = false
): RoomTimeline {
  return { roomId, events, truncated }
}

function renderRoom() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <RoomWorkspace roomId={roomId} />
    </NextIntlClientProvider>
  )
}

/** The live composer editor (mounts once the room detail has hydrated). */
async function composerEditor() {
  await waitFor(() => expect(composerHandle.current?.getEditor()).toBeTruthy())
  const editor = composerHandle.current?.getEditor()
  if (!editor) throw new Error("composer editor not mounted")
  return editor
}

/** insertContent dispatches a real transaction, so onChange fires. */
function typeInComposer(
  editor: NonNullable<ReturnType<RichComposerHandle["getEditor"]>>,
  text: string
) {
  act(() => {
    editor.commands.insertContent(text)
  })
}

/** Pick a row from the open `@` panel (mousedown keeps editor focus). */
async function pickMentionRow(name: string | RegExp) {
  const row = await screen.findByRole("option", { name }, { timeout: 5000 })
  act(() => {
    row.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, cancelable: true })
    )
  })
}

describe("RoomWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    composerHandle.current = null
    virtuaCtl.visibleAll = true
    virtuaCtl.visible = new Set()
    virtuaCtl.shift = false
    virtuaCtl.onScroll = null
    virtuaCtl.scrollToIndex.mockClear()
    platform.roomChanged.clear()
    runtime.byConversationId.clear()
    api.getCollaborationRoom.mockResolvedValue(roomDetail())
    api.markCollaborationRoomSeen.mockResolvedValue(roomDetail())
    api.postCollaborationRoomMessage.mockResolvedValue({
      eventId: "evt-reply",
      roomId,
      deliveries: [],
      affectedConversationIds: [101],
      deduplicated: false,
    })
  })

  it("pages older posts without marking the room seen again", async () => {
    api.getCollaborationRoomTimeline.mockImplementation(
      async (_id: string, _limit?: number, beforeEventId?: string | null) => {
        if (beforeEventId === "evt-new") {
          return timeline([event({ id: "evt-old", body: "older post" })], false)
        }
        return timeline([event()], true)
      }
    )

    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Load older posts" }))

    expect(await screen.findByText("older post")).toBeTruthy()
    expect(screen.getByText("newest post")).toBeTruthy()
    expect(
      screen.queryByRole("button", { name: "Load older posts" })
    ).toBeNull()

    expect(api.getCollaborationRoomTimeline).toHaveBeenNthCalledWith(
      2,
      roomId,
      undefined,
      "evt-new"
    )
    expect(api.markCollaborationRoomSeen).toHaveBeenCalledTimes(1)
    expect(api.markCollaborationRoomSeen).toHaveBeenCalledWith(roomId)
  })

  it("does not show owner ranks or @all/@human chips in the host composer", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    // The member count is its own <button> inside the subtitle now, so the
    // line's text is split across elements — match the two halves.
    expect(screen.getByText(/Uncategorized ·/)).toBeTruthy()
    expect(screen.getByRole("button", { name: "1 members" })).toBeTruthy()
    expect(screen.queryByText("You host this room")).toBeNull()
    expect(screen.queryByText("owner")).toBeNull()
    expect(screen.queryByRole("button", { name: "@human" })).toBeNull()
    // The dedicated @all chip was removed as redundant — the @ panel's own
    // "@all" option (see "scopes the @ panel to room members plus
    // @all/@human" below) already covers picking it.
    expect(screen.queryByRole("button", { name: "@all" })).toBeNull()
  })

  it("shows the Collection name instead of a Session folder breadcrumb", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      collectionId: 7,
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText(/Research ·/)).toBeTruthy()
    expect(screen.queryByText(/Uncategorized ·/)).toBeNull()
  })

  it("highlights the row of a message that mentions @human", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({ id: "evt-plain", body: "just fyi", mentionHuman: false }),
        event({
          id: "evt-mention",
          body: "wake up please",
          mentionHuman: true,
        }),
      ])
    )
    renderRoom()

    const plainRow = (await screen.findByText("just fyi")).closest("article")
    const mentionRow = screen.getByText("wake up please").closest("article")
    expect(plainRow?.getAttribute("data-mention-human")).toBeNull()
    expect(mentionRow?.getAttribute("data-mention-human")).toBe("true")
  })

  it("marks a member as running only while liveMessage is set", async () => {
    runtime.byConversationId.set(101, { liveMessage: { id: "live-1" } })
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Show members" }))
    expect(screen.getByText("Running")).toBeTruthy()
  })

  it("does not treat idle Sessions as running members", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    expect(screen.queryByText("Running")).toBeNull()
  })

  it("posts a reply against the selected event", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))

    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    const editor = await composerEditor()
    const reply = screen.getByRole("button", { name: "Reply" })
    expect(reply.className).not.toMatch(/absolute/)
    expect(reply.closest("div")?.textContent).toContain("Planner")
    fireEvent.click(reply)
    expect(screen.getByText("Replying to Planner")).toBeTruthy()
    // The wake discipline hint moved into the composer placeholder.
    expect(
      (editor.view.dom as HTMLElement)
        .querySelector("[data-placeholder]")
        ?.getAttribute("data-placeholder")
    ).toBe(
      "This reply only quotes Planner. It does not wake them. @ a Session to wake it."
    )

    typeInComposer(editor, "ack")
    fireEvent.click(screen.getByRole("button", { name: "Send" }))

    await waitFor(() => {
      expect(api.postCollaborationRoomMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          roomId,
          body: "ack",
          authorKind: "human",
          replyToEventId: "evt-new",
          invocationPolicy: "store_only",
          expectsReply: false,
        })
      )
    })
  })

  it("renders mentions inside the message instead of a header chip", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      members: [
        ...roomDetail().members,
        {
          conversationId: 202,
          title: "Session D",
          agentType: "claude_code",
          role: "member",
          joinedAt: "2026-08-01T00:00:00.000Z",
        },
      ],
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          body: "你好",
          mentionConversationIds: [202],
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText("你好")).toBeTruthy()
    const article = screen.getByRole("article")
    expect(within(article).getByText("@Session D")).toBeTruthy()
    expect(within(article).queryByText("@ Session D")).toBeNull()
    // Session mentions render as the same reference badge the Session
    // transcript uses.
    expect(
      article.querySelector('[data-reference-badge][data-ref-type="session"]')
    ).not.toBeNull()
  })

  it("keeps a typed @all inside the post body", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          body: "你们说说自己是谁 @all",
          mentionConversationIds: [101],
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText(/你们说说自己是谁/)).toBeTruthy()
    expect(within(screen.getByRole("article")).getByText("@all")).toBeTruthy()
  })

  it("inserts a structured @all badge when picked from the @ panel", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    const editor = await composerEditor()

    typeInComposer(editor, "@all")
    await pickMentionRow("@all")

    // The panel inserts the same structured `codeg://all` token the
    // now-removed dedicated chip used to insert — never bare prose.
    await waitFor(() => expect(editor.getText()).toContain("(codeg://all)"))
    // …and the wake preview says so before anything is sent.
    expect(screen.getByTestId("wake-preview").textContent).toContain("@all")

    fireEvent.click(screen.getByRole("button", { name: "Send" }))
    await waitFor(() => {
      expect(api.postCollaborationRoomMessage).toHaveBeenCalledWith(
        expect.objectContaining({ mentionAll: true, expectsReply: true })
      )
    })
  })

  it("quotes the original post on a reply in the same timeline", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({ id: "evt-old", body: "please review the plan" }),
        event({
          id: "evt-reply",
          body: "looks good",
          replyToEventId: "evt-old",
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText("looks good")).toBeTruthy()
    expect(screen.getAllByText("please review the plan").length).toBe(2)
    expect(screen.getAllByText("Planner").length).toBeGreaterThan(1)
  })

  it("says the original post is missing when it is not loaded", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          id: "evt-reply",
          body: "looks good",
          replyToEventId: "evt-gone",
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText("looks good")).toBeTruthy()
    expect(screen.getByText("Original post is not loaded")).toBeTruthy()
  })

  it("deletes the room through the API and closes its tab", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    api.deleteCollaborationRoom.mockResolvedValue(undefined)

    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "More actions" }))
    await user.click(
      await screen.findByRole("menuitem", { name: "Delete room" })
    )
    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByText("Delete this room?")).toBeTruthy()
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete room" }))

    await waitFor(() => {
      expect(api.deleteCollaborationRoom).toHaveBeenCalledWith(roomId)
    })
    expect(api.closeTab).toHaveBeenCalledWith(`room-${roomId}`)
    expect(api.refreshCatalog).toHaveBeenCalled()
  })

  it("adds an additional path through the manage-paths dialog", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    api.addCollaborationRoomPath.mockResolvedValue({
      ...roomDetail(),
      additionalPaths: [
        { id: 1, path: "/extra/notes", createdAt: "2026-08-20T00:00:00.000Z" },
      ],
    })

    renderRoom()
    expect(await screen.findByText("newest post")).toBeTruthy()

    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "More actions" }))
    await user.click(
      await screen.findByRole("menuitem", { name: "Manage paths" })
    )
    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByText("No additional paths yet.")).toBeTruthy()

    // The path box belongs to the shared DirectoryBrowser now; it opens on the
    // home directory, so clear that before typing an absolute path by hand.
    const pathBox = await within(dialog).findByDisplayValue("/home/dev")
    await user.clear(pathBox)
    await user.type(pathBox, "/extra/notes")
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }))

    await waitFor(() => {
      expect(api.addCollaborationRoomPath).toHaveBeenCalledWith(
        roomId,
        "/extra/notes"
      )
    })
    expect(await within(dialog).findByText("/extra/notes")).toBeTruthy()
  })

  it("removes an additional path through the manage-paths dialog", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      additionalPaths: [
        { id: 5, path: "/extra/notes", createdAt: "2026-08-20T00:00:00.000Z" },
      ],
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    api.removeCollaborationRoomPath.mockResolvedValue({
      ...roomDetail(),
      additionalPaths: [],
    })

    renderRoom()
    expect(await screen.findByText("newest post")).toBeTruthy()

    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "More actions" }))
    await user.click(
      await screen.findByRole("menuitem", { name: "Manage paths" })
    )
    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByText("/extra/notes")).toBeTruthy()

    fireEvent.click(within(dialog).getByRole("button", { name: "Remove path" }))

    await waitFor(() => {
      expect(api.removeCollaborationRoomPath).toHaveBeenCalledWith(roomId, 5)
    })
    expect(within(dialog).queryByText("/extra/notes")).toBeNull()
  })

  it("opens a Session from the timeline name and an @ mention", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      members: [
        ...roomDetail().members,
        {
          conversationId: 202,
          title: "Session D",
          agentType: "claude_code",
          role: "member",
          joinedAt: "2026-08-01T00:00:00.000Z",
        },
      ],
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          body: "please look",
          mentionConversationIds: [202],
        }),
      ])
    )
    renderRoom()
    expect(await screen.findByText("please look")).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Planner" }))
    expect(api.openTab).toHaveBeenCalledWith(7, 101, "codex", true, "Planner")

    fireEvent.click(
      within(screen.getByRole("article")).getByRole("button", {
        name: "session: @Session D",
      })
    )
    expect(api.openTab).toHaveBeenCalledWith(
      7,
      202,
      "claude_code",
      true,
      "Session D"
    )
  })

  it("scrolls to the quoted post when the quote block is clicked", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({ id: "evt-old", body: "please review the plan" }),
        event({
          id: "evt-reply",
          body: "looks good",
          replyToEventId: "evt-old",
        }),
      ])
    )
    const scrollIntoView = vi.fn()
    Element.prototype.scrollIntoView = scrollIntoView
    renderRoom()
    expect(await screen.findByText("looks good")).toBeTruthy()

    fireEvent.click(screen.getByTitle("Jump to quoted post"))
    expect(scrollIntoView).toHaveBeenCalled()
  })

  it("does not show a needs-reply switch in the host composer", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    expect(screen.queryByRole("checkbox", { name: "Needs a reply" })).toBeNull()
  })

  it("posts expectsReply when a Session is mentioned via the @ panel", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    const editor = await composerEditor()

    typeInComposer(editor, "@plan")
    await pickMentionRow(/Planner/)

    // The badge serializes to a structured session URI, and the preview
    // announces the wake before sending.
    await waitFor(() =>
      expect(editor.getText()).toContain("codeg://session/101")
    )
    expect(screen.getByTestId("wake-preview").textContent).toContain("Planner")

    fireEvent.click(screen.getByRole("button", { name: "Send" }))
    await waitFor(() => {
      expect(api.postCollaborationRoomMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          expectsReply: true,
          targetConversationIds: [101],
        })
      )
    })
  })

  it("scopes the @ panel to room members plus @all/@human", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    const editor = await composerEditor()

    typeInComposer(editor, "@")
    const popup = await screen.findByTestId("mention-popup", undefined, {
      timeout: 5000,
    })
    // Members and the two structured pseudo-mentions… (the panel's fetch is
    // debounced, so the first row lookup must be async)
    await within(popup).findByRole("option", { name: /Planner/ })
    expect(within(popup).getByRole("option", { name: "@all" })).toBeTruthy()
    expect(within(popup).getByRole("option", { name: "@human" })).toBeTruthy()
    // …but never a Session that is not in the room.
    expect(within(popup).queryByText("Session D")).toBeNull()
  })

  it("offers session/file/commit tabs but never an agent tab", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    const editor = await composerEditor()

    typeInComposer(editor, "@")
    const popup = await screen.findByTestId("mention-popup", undefined, {
      timeout: 5000,
    })
    await within(popup).findByRole("option", { name: /Planner/ })
    const tabs = within(popup).getAllByRole("tab")
    expect(tabs).toHaveLength(3)
    expect(tabs[0]).toHaveAccessibleName(/Sessions/)
    expect(tabs[1]).toHaveAccessibleName(/Files/)
    expect(tabs[2]).toHaveAccessibleName(/Commits/)
    // Members load first, so the session tab (not an agent tab, which the
    // room never offers) is the default-active one.
    expect(
      within(popup).getByRole("tab", { selected: true })
    ).toHaveAccessibleName(/Sessions/)
  })

  it("previews an empty wake list before anyone is mentioned", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    await composerEditor()
    expect(screen.getByTestId("wake-preview").textContent).toBe(
      "Plain message — recorded to the timeline, wakes no one"
    )
  })

  it("keeps the members panel collapsed until the header toggle is pressed", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")
    expect(screen.queryByLabelText("Add a Session")).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Show members" }))
    expect(screen.getByLabelText("Add a Session")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Hide members" }))
    expect(screen.queryByLabelText("Add a Session")).toBeNull()
  })

  it("also opens the members panel from the member-count text in the subtitle", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    await screen.findByText("newest post")

    expect(screen.queryByLabelText("Add a Session")).toBeNull()
    // The clickable entry point is the count itself, not the whole subtitle
    // line — "Uncategorized" stays plain text.
    fireEvent.click(screen.getByRole("button", { name: "1 members" }))
    expect(screen.getByLabelText("Add a Session")).toBeTruthy()

    // Same control toggles it closed again, like the header icon button.
    fireEvent.click(screen.getByRole("button", { name: "1 members" }))
    expect(screen.queryByLabelText("Add a Session")).toBeNull()
  })

  it("asks for confirmation before removing a member", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Show members" }))
    fireEvent.click(screen.getByRole("button", { name: "Remove from room" }))

    const dialog = await screen.findByRole("alertdialog")
    expect(
      within(dialog).getByText("Remove Planner from this room?")
    ).toBeTruthy()
    // The dialog only asks — nothing is removed until confirmed.
    expect(api.removeCollaborationRoomMember).not.toHaveBeenCalled()
  })

  it("keeps the member when the removal confirmation is canceled", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Show members" }))
    fireEvent.click(screen.getByRole("button", { name: "Remove from room" }))
    const dialog = await screen.findByRole("alertdialog")
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }))

    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
    expect(api.removeCollaborationRoomMember).not.toHaveBeenCalled()
    // The member row (and its remove affordance) is still there — canceling
    // left the roster untouched. ("Planner" itself is ambiguous here: the
    // open members panel and the timeline's sender name both show it.)
    expect(
      screen.getByRole("button", { name: "Remove from room" })
    ).toBeTruthy()
  })

  it("removes the member through the API once the confirmation is accepted", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    api.removeCollaborationRoomMember.mockResolvedValue({
      ...roomDetail(),
      members: [],
    })
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Show members" }))
    fireEvent.click(screen.getByRole("button", { name: "Remove from room" }))
    const dialog = await screen.findByRole("alertdialog")
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Remove from room" })
    )

    await waitFor(() => {
      expect(api.removeCollaborationRoomMember).toHaveBeenCalledWith(
        roomId,
        101
      )
    })
    expect(api.refreshCatalog).toHaveBeenCalled()
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
  })

  it("opens a find bar on Ctrl+F and walks the matching posts", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({ id: "evt-a", body: "please **review** the plan" }),
        event({ id: "evt-b", body: "second review pass" }),
      ])
    )
    renderRoom()
    expect(await screen.findByText("second review pass")).toBeTruthy()

    // No bar until the shortcut asks for one.
    expect(screen.queryByLabelText("Find in this room")).toBeNull()
    fireEvent.keyDown(document.body, { key: "f", ctrlKey: true })
    const input = await screen.findByLabelText("Find in this room")

    fireEvent.change(input, { target: { value: "review" } })
    const bar = input.closest("[data-conversation-find-bar]") as HTMLElement
    expect(within(bar).getByText("1 / 2")).toBeTruthy()

    // The selected match is marked on the post row, which is what the
    // highlighter scrolls to — the emphasis itself is a CSS Custom Highlight
    // range, and jsdom implements neither that API nor layout.
    await waitFor(() =>
      expect(
        document.querySelector("[data-conversation-find-current]")?.id
      ).toBe("room-event-evt-a")
    )

    fireEvent.click(screen.getByRole("button", { name: "Next match" }))
    expect(within(bar).getByText("2 / 2")).toBeTruthy()
    await waitFor(() =>
      expect(
        document.querySelector("[data-conversation-find-current]")?.id
      ).toBe("room-event-evt-b")
    )
  })

  it("leaves Ctrl+F alone for a Room tab that is not the active one", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <RoomWorkspace roomId={roomId} isActive={false} />
      </NextIntlClientProvider>
    )
    expect(await screen.findByText("newest post")).toBeTruthy()

    fireEvent.keyDown(document.body, { key: "f", ctrlKey: true })
    expect(screen.queryByLabelText("Find in this room")).toBeNull()
  })

  it("names the bound folder in the header, full path in the tooltip", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      rootFolderId: 7,
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    const folder = screen.getByTitle("/repo/research")
    expect(folder.textContent).toBe("research")
    expect(screen.queryByText("No bound folder")).toBeNull()
  })

  it("says so when the room has no bound folder", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    expect(screen.getByText("No bound folder")).toBeTruthy()
  })

  it("shows the bound folder and a directory browser in the paths dialog", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      rootFolderId: 7,
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    expect(await screen.findByText("newest post")).toBeTruthy()

    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "More actions" }))
    await user.click(
      await screen.findByRole("menuitem", { name: "Manage paths" })
    )
    const dialog = await screen.findByRole("dialog")

    // The dialog's own hint talks about "its own linked folder" — it now says
    // which one, with the full path on hover.
    expect(within(dialog).getByTitle("/repo/research")).toBeTruthy()
    // Typed absolute paths still work, but a folder can now be browsed to.
    expect(
      within(dialog).getByPlaceholderText("Enter directory path...")
    ).toBeTruthy()
    expect(
      await within(dialog).findByRole("button", { name: /notes/ })
    ).toBeTruthy()
  })

  it("refreshes the header and the @ scope when the room is moved into a Collection", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    expect(screen.getByText(/Uncategorized ·/)).toBeTruthy()
    expect(screen.getByText("No bound folder")).toBeTruthy()

    // What `collaboration_room_assign_collection` broadcasts once the move
    // lands (`publish_room` → ROOM_CHANGED_EVENT).
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      collectionId: 7,
      rootFolderId: 7,
    })
    await act(async () => {
      for (const handler of platform.roomChanged) {
        handler({ roomId, workbenchId: 1 })
      }
    })

    expect(await screen.findByText(/Research ·/)).toBeTruthy()
    // Same `rootFolderId` that scopes the `@` panel's file/commit search.
    expect(screen.getByTitle("/repo/research")).toBeTruthy()
    expect(screen.queryByText("No bound folder")).toBeNull()
  })

  it("shows how many of the asked Sessions have answered", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          body: "who is taking this?",
          expectsReply: true,
          expectedReplyCount: 3,
          resolvedReplyCount: 1,
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText("who is taking this?")).toBeTruthy()
    const badge = screen.getByText("1/3 answered")
    // The numbers are the glance; the sentence that explains them is a hover.
    expect(badge.getAttribute("title")).toBe(
      "Asked 3 Sessions · 1 answered · 2 still owe a reply"
    )
    expect(screen.queryByText("needs a reply")).toBeNull()
  })

  it("settles the badge once every asked Session has answered", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          body: "all done here",
          expectsReply: true,
          expectedReplyCount: 2,
          resolvedReplyCount: 2,
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText("all done here")).toBeTruthy()
    const badge = screen.getByText("2/2 answered")
    expect(badge.getAttribute("title")).toBe(
      "All 2 Sessions this post asked have answered"
    )
    // A paid ledger must stop reading as a warning.
    expect(badge.className).toContain("emerald")
    expect(badge.className).not.toContain("amber")
  })

  it("shows no reply badge at all on a post that asked nothing", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    expect(screen.queryByText("needs a reply")).toBeNull()
    expect(screen.queryByText(/answered/)).toBeNull()
  })

  it("keeps the plain wording for an ask with no delivery ledger", async () => {
    // `@human` is a flag on the event, not a fan-out target, so this ask has
    // no delivery rows behind it — "0/0 answered" would be nonsense. Older
    // payloads carry no counts at all and must land on the same wording.
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          id: "evt-human-ask",
          body: "human, thoughts?",
          expectsReply: true,
          mentionHuman: true,
          expectedReplyCount: 0,
          resolvedReplyCount: 0,
        }),
        event({
          id: "evt-legacy-ask",
          body: "older payload",
          expectsReply: true,
          createdAt: "2026-08-18T13:00:00.000Z",
        }),
      ])
    )
    renderRoom()

    expect(await screen.findByText("human, thoughts?")).toBeTruthy()
    const badges = screen.getAllByText("needs a reply")
    expect(badges).toHaveLength(2)
    for (const badge of badges) {
      expect(badge.getAttribute("title")).toBeNull()
    }
    expect(screen.queryByText(/answered/)).toBeNull()
  })

  it("stamps find row indices after date separators", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          id: "evt-a",
          body: "first day",
          createdAt: "2026-08-17T12:00:00.000Z",
        }),
        event({
          id: "evt-b",
          body: "second day",
          createdAt: "2026-08-18T12:00:00.000Z",
        }),
      ])
    )
    renderRoom()
    expect(await screen.findByText("second day")).toBeTruthy()
    expect(document.getElementById("room-event-evt-a")).toHaveAttribute(
      "data-find-row-index",
      "1"
    )
    expect(document.getElementById("room-event-evt-b")).toHaveAttribute(
      "data-find-row-index",
      "3"
    )
    const separators = document.querySelectorAll(
      "[data-transcript-date-separator]"
    )
    expect(separators).toHaveLength(2)
  })

  it("opens the timeline inside a stick-to-bottom scroller", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()
    expect(await screen.findByText("newest post")).toBeTruthy()
    const scroller = screen.getByRole("log")
    expect(scroller).toContainElement(screen.getByText("newest post"))
  })

  it("scrolls an off-screen Room find hit into view and highlights it", async () => {
    virtuaCtl.visibleAll = false
    // Loader absent (not truncated). Mixed stream is [sep, evt-a, evt-b];
    // only the last virtua child (evt-b) is mounted until find locates evt-a.
    virtuaCtl.visible = new Set([2])
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline([
        event({
          id: "evt-a",
          body: "hidden needle post",
          createdAt: "2026-08-18T12:00:00.000Z",
        }),
        event({
          id: "evt-b",
          body: "visible tail",
          createdAt: "2026-08-18T13:00:00.000Z",
        }),
      ])
    )
    const registry = { set: vi.fn(), delete: vi.fn() }
    class TestHighlight {}
    Object.defineProperty(globalThis.CSS, "highlights", {
      configurable: true,
      value: registry,
    })
    Object.defineProperty(globalThis, "Highlight", {
      configurable: true,
      value: TestHighlight,
    })

    renderRoom()
    expect(await screen.findByText("visible tail")).toBeTruthy()
    expect(screen.queryByText("hidden needle post")).toBeNull()

    fireEvent.keyDown(document.body, { key: "f", ctrlKey: true })
    const input = await screen.findByLabelText("Find in this room")
    fireEvent.change(input, { target: { value: "needle" } })

    await waitFor(() => {
      expect(virtuaCtl.scrollToIndex).toHaveBeenCalled()
    })
    await waitFor(() => {
      expect(screen.getByText("hidden needle post")).toBeTruthy()
    })
    await waitFor(() => {
      expect(
        document.querySelector("[data-conversation-find-current]")?.id
      ).toBe("room-event-evt-a")
    })
    expect(registry.set).toHaveBeenCalled()
    document.getElementById("codeg-conversation-find-styles")?.remove()
  })

  it("keeps the viewport anchored and date separators correct after prepend", async () => {
    api.getCollaborationRoomTimeline.mockImplementation(
      async (_id: string, _limit?: number, beforeEventId?: string | null) => {
        if (beforeEventId === "evt-new") {
          return timeline(
            [
              event({
                id: "evt-old",
                body: "older post",
                createdAt: "2026-08-17T12:00:00.000Z",
              }),
            ],
            false
          )
        }
        return timeline(
          [
            event({
              id: "evt-new",
              body: "newest post",
              createdAt: "2026-08-18T12:00:00.000Z",
            }),
          ],
          true
        )
      }
    )

    renderRoom()
    expect(await screen.findByText("newest post")).toBeTruthy()
    expect(
      document.querySelectorAll("[data-transcript-date-separator]")
    ).toHaveLength(1)

    fireEvent.click(screen.getByRole("button", { name: "Load older posts" }))

    expect(await screen.findByText("older post")).toBeTruthy()
    expect(screen.getByText("newest post")).toBeTruthy()
    expect(virtuaCtl.shift).toBe(true)
    const separators = document.querySelectorAll(
      "[data-transcript-date-separator]"
    )
    expect(separators).toHaveLength(2)
    expect(document.getElementById("room-event-evt-old")).toHaveAttribute(
      "data-find-row-index",
      "1"
    )
    expect(document.getElementById("room-event-evt-new")).toHaveAttribute(
      "data-find-row-index",
      "3"
    )
  })

  it("stops find-driven paging when the oldest id does not move", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(
      timeline(
        [event({ id: "evt-new", body: "needle stays at the tail" })],
        true
      )
    )
    renderRoom()
    expect(await screen.findByText("needle stays at the tail")).toBeTruthy()
    const initialCalls = api.getCollaborationRoomTimeline.mock.calls.length

    fireEvent.keyDown(document.body, { key: "f", ctrlKey: true })
    const input = await screen.findByLabelText("Find in this room")
    fireEvent.change(input, { target: { value: "needle" } })

    await waitFor(() => {
      expect(api.getCollaborationRoomTimeline.mock.calls.length).toBe(
        initialCalls + 1
      )
    })
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 40))
    })
    expect(api.getCollaborationRoomTimeline.mock.calls.length).toBe(
      initialCalls + 1
    )
  })
})

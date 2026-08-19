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

const api = vi.hoisted(() => ({
  getCollaborationRoom: vi.fn(),
  getCollaborationRoomTimeline: vi.fn(),
  markCollaborationRoomSeen: vi.fn(),
  postCollaborationRoomMessage: vi.fn(),
  refreshCatalog: vi.fn(),
  deleteCollaborationRoom: vi.fn(),
  closeTab: vi.fn(),
  openTab: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  getCollaborationRoom: api.getCollaborationRoom,
  getCollaborationRoomTimeline: api.getCollaborationRoomTimeline,
  markCollaborationRoomSeen: api.markCollaborationRoomSeen,
  postCollaborationRoomMessage: api.postCollaborationRoomMessage,
  addCollaborationRoomMembers: vi.fn(),
  removeCollaborationRoomMember: vi.fn(),
  renameCollaborationRoom: vi.fn(),
  deleteCollaborationRoom: api.deleteCollaborationRoom,
}))

vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn().mockResolvedValue(() => {}),
  // `useReferenceSearch` (now wired into the room's `@` panel) pulls in
  // `useAcpAgents`, which calls this unconditionally on mount — see
  // automations-page.test.tsx / task-detail-sheet.follow-up.test.tsx for the
  // same mock, required for the same reason.
  onTransportReconnect: vi.fn(() => () => {}),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({ openTab: api.openTab, closeTab: api.closeTab }),
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
      // None of these fixtures set `rootFolderId` on the room detail, so
      // `roomFolderPath` resolution never looks this up — kept here (rather
      // than omitted) to match the real store's shape.
      allFolders: [],
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
    expect(screen.getByText("Uncategorized · 1 members")).toBeTruthy()
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

    expect(await screen.findByText("Research · 1 members")).toBeTruthy()
    expect(screen.queryByText("Uncategorized · 1 members")).toBeNull()
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
})

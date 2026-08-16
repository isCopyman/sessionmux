import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"

const tabs = vi.hoisted(() => ({ openTab: vi.fn() }))
const workspace = vi.hoisted(() => ({
  conversations: [
    { id: 1, folder_id: 10, title: "Reviewer", agent_type: "codex" },
  ],
}))
const replyDialog = vi.hoisted(() => ({ props: null as unknown }))

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) =>
    values?.name ? `${key}:${values.name}` : key,
}))
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => tabs,
}))
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: typeof workspace) => unknown) =>
    selector(workspace),
}))
vi.mock("@/components/collaboration/session-message-composer-dialog", () => ({
  SessionMessageComposerDialog: (props: unknown) => {
    replyDialog.props = props
    return <div data-testid="reply-dialog" />
  },
}))
vi.mock("./content-parts-renderer", () => ({
  ContentPartsRenderer: ({ parts }: { parts: Array<{ text?: string }> }) => (
    <p>{parts.map((part) => part.text).join("")}</p>
  ),
}))

import { CollaborationMessageCard } from "./collaboration-message-card"

function delivery(): CollaborationDelivery {
  return {
    id: "delivery-1",
    eventId: "event-1",
    source: {
      conversationId: 1,
      title: "Reviewer",
      agentType: "codex",
      folderPath: "/repo",
      backend: "current",
    },
    target: { conversationId: 2, backend: "current" },
    body: "Please check the proof.",
    replyToEventId: null,
    expectsReply: true,
    replyReceived: false,
    urgency: "normal",
    invocationPolicy: "store_only",
    deliveryHint: "default",
    state: "embedded",
    attentionState: "unread",
    openedAt: null,
    agentReceivedAt: "2026-08-16T00:01:00Z",
    agentReceiptKind: "managed_acp",
    agentReceiptRef: "turn-1",
    obligationState: "awaiting_reply",
    obligationCreatedAt: "2026-08-16T00:00:00Z",
    obligationResolvedAt: null,
    uiSeenAt: null,
    embeddedTurnRef: "turn-1",
    attempts: 1,
    error: null,
    createdAt: "2026-08-16T00:00:00Z",
    updatedAt: "2026-08-16T00:01:00Z",
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  replyDialog.props = null
})

describe("CollaborationMessageCard", () => {
  it("shows distinct lifecycle facts and keeps navigation/reply linked", () => {
    render(
      <CollaborationMessageCard
        delivery={delivery()}
        currentConversationId={2}
      />
    )

    expect(screen.getByText("transcriptFrom:Reviewer")).toBeInTheDocument()
    expect(screen.getByText("Please check the proof.")).toBeInTheDocument()
    expect(screen.getByText("unreadCount")).toBeInTheDocument()
    expect(screen.getByText("stateEmbedded")).toBeInTheDocument()
    expect(screen.getByText("stateNeedsReply")).toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "openSession" }))
    expect(tabs.openTab).toHaveBeenCalledWith(10, 1, "codex", true, "Reviewer")

    fireEvent.click(screen.getByRole("button", { name: "reply" }))
    expect(replyDialog.props).toEqual(
      expect.objectContaining({
        sourceConversationId: 2,
        initialTargetConversationId: 1,
        replyToEventId: "event-1",
        open: true,
      })
    )
  })
})

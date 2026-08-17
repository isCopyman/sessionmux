import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"

const tabs = vi.hoisted(() => ({ openTab: vi.fn() }))
const workspace = vi.hoisted(() => ({
  conversations: [
    { id: 1, folder_id: 10, title: "Reviewer", agent_type: "codex" },
    { id: 2, folder_id: 10, title: "Worker", agent_type: "claude_code" },
  ],
}))
const api = vi.hoisted(() => ({ markCollaborationSeen: vi.fn() }))

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) =>
    values?.id != null
      ? `${key}:${values.id}`
      : values?.name
        ? `${key}:${values.name}`
        : key,
}))
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => tabs,
}))
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: typeof workspace) => unknown) =>
    selector(workspace),
}))
vi.mock("@/lib/api", () => api)
vi.mock("./content-parts-renderer", () => ({
  ContentPartsRenderer: ({ parts }: { parts: Array<{ text?: string }> }) => (
    <p>{parts.map((part) => part.text).join("")}</p>
  ),
}))
vi.mock("./collapsible-user-message", () => ({
  CollapsibleUserMessage: ({
    parts,
  }: {
    parts: Array<{ text?: string }>
  }) => <p>{parts.map((part) => part.text).join("")}</p>,
}))

import { CollaborationMessageCard } from "./collaboration-message-card"

function delivery(
  overrides: Partial<CollaborationDelivery> = {}
): CollaborationDelivery {
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
    target: {
      conversationId: 2,
      title: "Worker",
      agentType: "claude_code",
      folderPath: "/repo",
      backend: "current",
    },
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
    ...overrides,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  api.markCollaborationSeen.mockResolvedValue({})
})

describe("CollaborationMessageCard", () => {
  it("renders inbound mail as a user prompt with a Session source mark", () => {
    render(
      <CollaborationMessageCard
        delivery={delivery()}
        currentConversationId={2}
      />
    )

    expect(screen.getByText("fromSession:Reviewer")).toBeInTheDocument()
    expect(screen.getByText("Please check the proof.")).toBeInTheDocument()
    expect(
      document.querySelector("[data-collaboration-direction='inbound']")
    ).not.toBeNull()

    fireEvent.click(screen.getByRole("button", { name: "openSession" }))
    expect(tabs.openTab).toHaveBeenCalledWith(10, 1, "codex", true, "Reviewer")
  })
})

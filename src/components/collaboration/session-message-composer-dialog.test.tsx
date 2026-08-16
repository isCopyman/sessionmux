import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

const api = vi.hoisted(() => ({ send: vi.fn(), sendInterrupt: vi.fn() }))
const onOpenChange = vi.fn()

vi.mock("@/lib/api", () => ({
  sendCollaborationMessage: api.send,
  sendAndInterruptCollaborationMessage: api.sendInterrupt,
}))
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}))
vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) =>
    values
      ? `${key}:${Object.entries(values)
          .map(([name, value]) => `${name}=${value}`)
          .join(",")}`
      : key,
}))
vi.mock("@/components/agent-icon", () => ({
  AgentIcon: () => <span data-testid="agent-icon" />,
}))
vi.mock("@/components/ui/scroll-area", () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}))

const store = {
  conversations: [
    {
      id: 1,
      folder_id: 10,
      title: "Source",
      agent_type: "codex",
      kind: "regular",
      updated_at: "2026-08-16T03:00:00Z",
    },
    {
      id: 2,
      folder_id: 10,
      title: "Reviewer",
      agent_type: "claude_code",
      kind: "regular",
      updated_at: "2026-08-16T02:00:00Z",
    },
    {
      id: 3,
      folder_id: 11,
      title: "Researcher",
      agent_type: "gemini",
      kind: "regular",
      updated_at: "2026-08-16T01:00:00Z",
    },
  ],
  folders: [
    { id: 10, name: "Thesis", path: "/thesis" },
    { id: 11, name: "Notes", path: "/notes" },
  ],
}

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: typeof store) => unknown) =>
    selector(store),
}))

import { SessionMessageComposerDialog } from "./session-message-composer-dialog"

beforeEach(() => {
  vi.clearAllMocks()
  api.send.mockResolvedValue({
    eventId: "event-1",
    affectedConversationIds: [1, 2, 3],
    deduplicated: false,
    deliveries: [
      { id: "d2", state: "pending" },
      { id: "d3", state: "pending" },
    ],
  })
  api.sendInterrupt.mockResolvedValue({
    message: { eventId: "event-1", deliveries: [] },
    interrupt: {
      deduplicated: false,
      operation: { id: "interrupt-1", state: "waiting_for_terminal" },
    },
    interruptError: null,
  })
})

describe("SessionMessageComposerDialog", () => {
  it("uses an explicit multi-target operation and stable numeric addresses", async () => {
    render(
      <SessionMessageComposerDialog
        sourceConversationId={1}
        open
        onOpenChange={onOpenChange}
      />
    )

    expect(screen.queryByText("Source")).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: /Reviewer/ }))
    fireEvent.click(screen.getByRole("button", { name: /Researcher/ }))
    fireEvent.change(screen.getByPlaceholderText("bodyPlaceholder"), {
      target: { value: "Please compare these claims" },
    })
    fireEvent.click(screen.getByRole("button", { name: /^send$/ }))

    await waitFor(() => expect(api.send).toHaveBeenCalledTimes(1))
    expect(api.send).toHaveBeenCalledWith(
      expect.objectContaining({
        sourceConversationId: 1,
        targetConversationIds: [2, 3],
        body: "Please compare these claims",
        clientDedupeId: expect.any(String),
        invocationPolicy: "store_only",
        deliveryHint: "default",
      })
    )
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("searches by folder and keeps send disabled without a target", () => {
    render(
      <SessionMessageComposerDialog
        sourceConversationId={1}
        open
        onOpenChange={onOpenChange}
      />
    )
    fireEvent.change(screen.getByPlaceholderText("targetSearchPlaceholder"), {
      target: { value: "Notes" },
    })
    expect(screen.getByText("Researcher")).toBeInTheDocument()
    expect(screen.queryByText("Reviewer")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: /^send$/ })).toBeDisabled()
  })

  it("can queue the message for the target agent without interrupting its turn", async () => {
    render(
      <SessionMessageComposerDialog
        sourceConversationId={1}
        open
        onOpenChange={onOpenChange}
      />
    )
    fireEvent.click(screen.getByRole("radio", { name: "invokeWhenIdle" }))
    fireEvent.click(screen.getByRole("button", { name: /Reviewer/ }))
    fireEvent.change(screen.getByPlaceholderText("bodyPlaceholder"), {
      target: { value: "Read this after the current turn" },
    })
    fireEvent.click(screen.getByRole("button", { name: /^send$/ }))

    await waitFor(() => expect(api.send).toHaveBeenCalledTimes(1))
    expect(api.send).toHaveBeenCalledWith(
      expect.objectContaining({
        invocationPolicy: "invoke_when_idle",
        deliveryHint: "default",
      })
    )
  })

  it("can request non-destructive native steering with durable queue fallback", async () => {
    render(
      <SessionMessageComposerDialog
        sourceConversationId={1}
        open
        onOpenChange={onOpenChange}
      />
    )
    fireEvent.click(screen.getByRole("radio", { name: "steerIfSupported" }))
    fireEvent.click(screen.getByRole("button", { name: /Reviewer/ }))
    fireEvent.change(screen.getByPlaceholderText("bodyPlaceholder"), {
      target: {
        value: "Use this correction if the current turn can accept it",
      },
    })
    fireEvent.click(screen.getByRole("button", { name: /^send$/ }))

    await waitFor(() => expect(api.send).toHaveBeenCalledTimes(1))
    expect(api.send).toHaveBeenCalledWith(
      expect.objectContaining({
        invocationPolicy: "invoke_when_idle",
        deliveryHint: "steer_if_supported",
      })
    )
  })

  it("persists one target before requesting a separate stop operation", async () => {
    render(
      <SessionMessageComposerDialog
        sourceConversationId={1}
        open
        onOpenChange={onOpenChange}
      />
    )
    fireEvent.click(screen.getByRole("button", { name: /Reviewer/ }))
    fireEvent.click(screen.getByRole("button", { name: /Researcher/ }))
    fireEvent.click(screen.getByRole("radio", { name: "interruptCurrentTask" }))
    fireEvent.change(screen.getByPlaceholderText("bodyPlaceholder"), {
      target: { value: "Stop and review this correction" },
    })
    fireEvent.click(screen.getByRole("button", { name: /^send$/ }))

    await waitFor(() => expect(api.sendInterrupt).toHaveBeenCalledTimes(1))
    expect(api.send).not.toHaveBeenCalled()
    expect(api.sendInterrupt).toHaveBeenCalledWith(
      expect.objectContaining({
        message: expect.objectContaining({
          targetConversationIds: [2],
          invocationPolicy: "invoke_when_idle",
          deliveryHint: "default",
        }),
        interruptClientDedupeId: expect.any(String),
      })
    )
  })
})

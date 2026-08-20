import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

// Drives the REAL Streamdown pipeline (no streamdown mock): the point of these
// tests is that Room posts render through the SAME parse + sanitize + harden
// stack the Session transcript uses, so mocking it away would test nothing.
// Only the leaf dependencies of the link-safety hook are stubbed — the same set
// message-windows-path.test.tsx stubs, for the same reason.
const mocks = vi.hoisted(() => ({
  openFilePreview: vi.fn(),
  openUrl: vi.fn(),
  toastError: vi.fn(),
}))

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}))

vi.mock("sonner", () => ({
  toast: { error: mocks.toastError },
}))

vi.mock("@/lib/platform", () => ({
  openUrl: mocks.openUrl,
}))

vi.mock("@/lib/transport", () => ({
  isDesktop: () => false,
  getActiveRemoteConnectionId: () => null,
}))

vi.mock("@/contexts/active-folder-context", () => ({
  useActiveFolder: () => ({ activeFolder: { path: "/repo" } }),
}))

vi.mock("@/contexts/workspace-context", () => ({
  useWorkspaceActions: () => ({ openFilePreview: mocks.openFilePreview }),
}))

import { RoomPostBody } from "./room-post-body"
import {
  roomMessageMarkdown,
  type RoomBodyInput,
} from "@/lib/room-message-body"

/** A post body serialized exactly the way the timeline serializes it. */
function postSource(body: string, overrides: Partial<RoomBodyInput> = {}) {
  return roomMessageMarkdown({
    body,
    members: [{ conversationId: 202, title: "Session D" }],
    mentionConversationIds: [],
    allLabel: "@all",
    humanLabel: "@human",
    untitled: (id) => `#${id}`,
    ...overrides,
  })
}

describe("RoomPostBody", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.openFilePreview.mockResolvedValue(undefined)
  })

  it("renders a fenced block as code instead of flat text", async () => {
    const source = postSource(["```ts", "const a = 1", "```"].join("\n"))
    const { container } = render(<RoomPostBody source={source} />)

    await waitFor(() => expect(container.querySelector("code")).not.toBeNull())
    expect(container.textContent).toContain("const a = 1")
    expect(container.textContent).not.toContain("```")
  })

  it("renders bold, a list and a heading", async () => {
    const source = postSource(
      ["## Plan", "", "- **ship** it", "- then rest"].join("\n")
    )
    const { container } = render(<RoomPostBody source={source} />)

    await waitFor(() =>
      expect(container.querySelector("strong")).not.toBeNull()
    )
    expect(container.querySelector("strong")?.textContent).toBe("ship")
    expect(container.querySelector("h2")?.textContent).toBe("Plan")
    expect(container.querySelectorAll("li")).toHaveLength(2)
    expect(container.textContent).not.toContain("**")
  })

  it("lets the safety plugins strip a dangerous link but keeps a real one", async () => {
    const source = postSource(
      "[run](javascript:alert%28document.domain%29) and [docs](https://example.com)"
    )
    const { container } = render(<RoomPostBody source={source} />)

    await waitFor(() =>
      expect(container.querySelector('[data-streamdown="link"]')).not.toBeNull()
    )
    // The http link survives as a safety-routed button…
    expect(screen.getByRole("button", { name: /docs/ })).toBeTruthy()
    // …while the javascript: url reaches no attribute the browser would follow
    // (sanitize drops the href, harden then refuses to transform the link).
    expect(container.innerHTML).not.toContain('href="javascript')
    expect(container.querySelector('[title*="javascript"]')).toBeNull()
    const links = container.querySelectorAll('[data-streamdown="link"]')
    expect(links).toHaveLength(1)
  })

  it("keeps the mention chips, clickable, alongside the markdown", async () => {
    const onOpenSession = vi.fn()
    const source = postSource("@Session D please review **this**", {
      mentionHuman: true,
    })
    const { container } = render(
      <RoomPostBody source={source} onOpenSession={onOpenSession} />
    )

    await waitFor(() =>
      expect(container.querySelector("strong")).not.toBeNull()
    )
    // The session chip is the same reference badge the Session transcript uses…
    expect(
      container.querySelector('[data-reference-badge][data-ref-type="session"]')
    ).not.toBeNull()
    // …and clicking it still opens the Session it points at.
    fireEvent.click(screen.getByRole("button", { name: "session: @Session D" }))
    expect(onOpenSession).toHaveBeenCalledWith(202)
    // The @human pill comes from the event metadata, not from the body text.
    expect(screen.getByText("@human")).toBeTruthy()
  })

  it("keeps a typed @all as a pill, not as a link", async () => {
    const source = postSource("status check @all")
    const { container } = render(<RoomPostBody source={source} />)

    await waitFor(() => expect(container.textContent).toContain("status check"))
    expect(screen.getByText("@all")).toBeTruthy()
    expect(container.querySelector('[data-streamdown="link"]')).toBeNull()
    expect(container.innerHTML).not.toContain("codeg://all")
  })

  it("leaves a mention inside a code fence as literal code", async () => {
    const body = ["```", "notify @all here", "```"].join("\n")
    const source = postSource(body)
    // The serializer masks fenced spans, so the fence round-trips byte-exact.
    expect(source).toBe(body)

    const { container } = render(<RoomPostBody source={source} />)
    await waitFor(() => expect(container.querySelector("code")).not.toBeNull())
    expect(container.textContent).toContain("notify @all here")
    expect(container.querySelector("[data-reference-badge]")).toBeNull()
  })

  it("keeps a closing fence intact when a metadata mention is appended", async () => {
    const source = postSource(["here:", "```", "patch", "```"].join("\n"), {
      mentionConversationIds: [202],
    })
    const { container } = render(<RoomPostBody source={source} />)

    await waitFor(() => expect(container.querySelector("code")).not.toBeNull())
    const badge = container.querySelector('[data-ref-type="session"]')
    expect(badge).not.toBeNull()
    // The chip sits after the code block, never inside it — an appended chip on
    // the closing-fence line would stop it closing and swallow the rest.
    expect(badge?.closest("pre")).toBeNull()
    expect(container.textContent).toContain("patch")
    expect(container.textContent).not.toContain("codeg://session")
  })

  it("shows a plain post exactly as written, line breaks included", async () => {
    const body = "just fyi\nsecond line, nothing fancy"
    const source = postSource(body)
    // No chips means no rewriting at all.
    expect(source).toBe(body)

    const { container } = render(<RoomPostBody source={source} />)
    await waitFor(() => expect(container.textContent).toContain("just fyi"))
    expect(container.textContent).toContain("second line, nothing fancy")
    // A single newline stays a line break, the way the pre-Markdown timeline
    // rendered it — a Room post is written in a chat composer.
    expect(container.querySelector("br")).not.toBeNull()
    expect(container.querySelector("strong, em, code, a")).toBeNull()
  })
})

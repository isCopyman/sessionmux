import { useState, type ReactElement } from "react"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { SessionPicker } from "./session-picker"
import enMessages from "@/i18n/messages/en.json"
import type { DbConversationSummary } from "@/lib/types"

const h = vi.hoisted(() => ({
  folders: [
    { id: 7, name: "codeg", alias: "FDF综述" },
    { id: 8, name: "notes", alias: null as string | null },
  ],
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (
    selector: (state: { folders: typeof h.folders }) => unknown
  ) => selector({ folders: h.folders }),
}))

function conversation(
  id: number,
  title: string | null,
  overrides: Partial<DbConversationSummary> = {}
): DbConversationSummary {
  return {
    id,
    folder_id: 7,
    title,
    title_locked: true,
    agent_type: "codex",
    status: "in_progress",
    kind: "regular",
    model: null,
    git_branch: null,
    external_id: null,
    message_count: 1,
    child_count: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: new Date(Date.now() - 3 * 60 * 60 * 1000).toISOString(),
    archived_at: null,
    pinned_at: null,
    ...overrides,
  }
}

function renderPicker(ui: ReactElement): ReturnType<typeof render> {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {ui}
    </NextIntlClientProvider>
  )
}

function SingleHarness({
  candidates,
  excludeIds,
}: {
  candidates: DbConversationSummary[]
  excludeIds?: number[]
}) {
  const [query, setQuery] = useState("")
  const [value, setValue] = useState<number | null>(null)
  return (
    <SessionPicker
      mode="single"
      candidates={candidates}
      value={value}
      onChange={setValue}
      query={query}
      onQueryChange={setQuery}
      emptyLabel="No Sessions available."
      excludeIds={excludeIds}
    />
  )
}

function MultiHarness({
  candidates,
  excludeIds,
}: {
  candidates: DbConversationSummary[]
  excludeIds?: number[]
}) {
  const [query, setQuery] = useState("")
  const [value, setValue] = useState<number[]>([])
  return (
    <SessionPicker
      mode="multi"
      candidates={candidates}
      value={value}
      onChange={setValue}
      query={query}
      onQueryChange={setQuery}
      emptyLabel="Every open Session is already in this room."
      excludeIds={excludeIds}
    />
  )
}

describe("SessionPicker", () => {
  beforeEach(() => {
    h.folders.splice(
      0,
      h.folders.length,
      { id: 7, name: "codeg", alias: "FDF综述" },
      { id: 8, name: "notes", alias: null }
    )
  })

  it("selects a single initiator via radio", async () => {
    const user = userEvent.setup()
    const { container } = renderPicker(
      <SingleHarness
        candidates={[conversation(1, "Alpha"), conversation(2, "Beta")]}
      />
    )

    const alpha = screen.getByRole("radio", { name: /Alpha/ })
    const beta = screen.getByRole("radio", { name: /Beta/ })
    expect(alpha).not.toBeChecked()
    expect(beta).not.toBeChecked()

    await user.click(beta)
    expect(screen.getByRole("radio", { name: /Beta/ })).toBeChecked()
    expect(screen.getByRole("radio", { name: /Alpha/ })).not.toBeChecked()
    expect(container.querySelectorAll("button button")).toHaveLength(0)
  })

  it("toggles multi selection via checkbox", async () => {
    const user = userEvent.setup()
    const { container } = renderPicker(
      <MultiHarness
        candidates={[conversation(1, "Alpha"), conversation(2, "Beta")]}
      />
    )

    await user.click(screen.getByRole("checkbox", { name: /Alpha/ }))
    expect(screen.getByRole("checkbox", { name: /Alpha/ })).toBeChecked()
    expect(screen.getByRole("checkbox", { name: /Beta/ })).not.toBeChecked()

    await user.click(screen.getByRole("checkbox", { name: /Beta/ }))
    expect(screen.getByRole("checkbox", { name: /Beta/ })).toBeChecked()

    await user.click(screen.getByRole("checkbox", { name: /Alpha/ }))
    expect(screen.getByRole("checkbox", { name: /Alpha/ })).not.toBeChecked()
    expect(container.querySelectorAll("button button")).toHaveLength(0)
  })

  it("filters rows by the built-in search box", async () => {
    const user = userEvent.setup()
    renderPicker(
      <MultiHarness
        candidates={[conversation(1, "Alpha"), conversation(2, "Beta")]}
      />
    )

    await user.type(screen.getByPlaceholderText("Search Sessions"), "bet")
    expect(screen.queryByRole("checkbox", { name: /Alpha/ })).toBeNull()
    expect(screen.getByRole("checkbox", { name: /Beta/ })).toBeTruthy()
  })

  it("hides ids listed in excludeIds", () => {
    renderPicker(
      <MultiHarness
        candidates={[conversation(1, "Alpha"), conversation(2, "Beta")]}
        excludeIds={[1]}
      />
    )

    expect(screen.queryByRole("checkbox", { name: /Alpha/ })).toBeNull()
    expect(screen.getByRole("checkbox", { name: /Beta/ })).toBeTruthy()
  })

  it("shows folder alias and relative time to disambiguate duplicate titles", () => {
    const { container } = renderPicker(
      <MultiHarness
        candidates={[
          conversation(1, "FDF综述"),
          conversation(2, "FDF综述", { folder_id: 8 }),
          conversation(3, null),
        ]}
      />
    )

    expect(screen.getAllByText("FDF综述")).toHaveLength(2)
    expect(screen.getAllByText("FDF综述 · 3h")).toHaveLength(2)
    expect(screen.getByText("notes · 3h")).toBeTruthy()
    expect(screen.getByText("Untitled session #3")).toBeTruthy()
    expect(container.querySelectorAll("button button")).toHaveLength(0)
  })
})

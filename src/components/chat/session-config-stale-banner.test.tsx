import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { SessionConfigStaleBanner } from "./session-config-stale-banner"
const connection = vi.hoisted(() => ({
  configStale: true,
  configStaleKind: "agent_config" as const,
  isViewer: false,
  isDelegationChild: false,
  status: "connected" as string,
  reapplyConfig: vi.fn(async () => true),
}))
vi.mock("@/hooks/use-connection", () => ({ useConnection: () => connection }))
vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: { queueDepth?: number }) =>
    values?.queueDepth == null ? key : `${key}:${values.queueDepth}`,
}))
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }))
function restart(queueDepth = 0) {
  return (
    <SessionConfigStaleBanner
      contextKey="tab-1"
      queueDepth={queueDepth}
      queueHydrated
    />
  )
}
describe("SessionConfigStaleBanner", () => {
  beforeEach(() => {
    connection.status = "connected"
    connection.reapplyConfig.mockReset().mockResolvedValue(true)
  })
  it("restarts an idle session once without opening a dialog", async () => {
    const view = render(restart())
    await waitFor(() => expect(connection.reapplyConfig).toHaveBeenCalledOnce())
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    connection.status = "connecting"
    view.rerender(restart())
    expect(connection.reapplyConfig).toHaveBeenCalledOnce()
  })
  it("explains a busy restart and applies Restart now", async () => {
    connection.status = "prompting"
    const user = userEvent.setup()
    render(restart(2))
    const dialog = await screen.findByRole("alertdialog")
    expect(dialog).toHaveTextContent("restartDescription:2")
    expect(within(dialog).getAllByRole("button")).toHaveLength(2)
    await user.click(within(dialog).getByRole("button", { name: "restartNow" }))
    await waitFor(() => expect(connection.reapplyConfig).toHaveBeenCalledOnce())
  })
  it("defers a queued restart until the next idle moment", async () => {
    const user = userEvent.setup()
    const view = render(restart(2))
    const dialog = await screen.findByRole("alertdialog")
    await user.click(
      within(dialog).getByRole("button", { name: "restartAfterTurn" })
    )
    expect(connection.reapplyConfig).not.toHaveBeenCalled()
    view.rerender(restart())
    await waitFor(() => expect(connection.reapplyConfig).toHaveBeenCalledOnce())
  })
})

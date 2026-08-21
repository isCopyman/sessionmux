import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it } from "vitest"

import {
  CreateRoomDialogProvider,
  useCreateRoomDialog,
} from "./create-room-dialog-context"

function Probe() {
  const { open, folderScopeId, openForFolder, setOpen } = useCreateRoomDialog()
  return (
    <div>
      <span data-testid="open">{String(open)}</span>
      <span data-testid="scope">
        {folderScopeId == null ? "none" : String(folderScopeId)}
      </span>
      <button type="button" onClick={() => openForFolder(7)}>
        open folder
      </button>
      <button type="button" onClick={() => setOpen(false)}>
        close
      </button>
      <button type="button" onClick={() => setOpen(true)}>
        open global
      </button>
    </div>
  )
}

describe("CreateRoomDialogProvider", () => {
  it("openForFolder sets the folder scope and setOpen(false) clears it", async () => {
    const user = userEvent.setup()
    render(
      <CreateRoomDialogProvider>
        <Probe />
      </CreateRoomDialogProvider>
    )

    expect(screen.getByTestId("open").textContent).toBe("false")
    expect(screen.getByTestId("scope").textContent).toBe("none")

    await user.click(screen.getByRole("button", { name: "open folder" }))
    expect(screen.getByTestId("open").textContent).toBe("true")
    expect(screen.getByTestId("scope").textContent).toBe("7")

    await user.click(screen.getByRole("button", { name: "close" }))
    expect(screen.getByTestId("open").textContent).toBe("false")
    expect(screen.getByTestId("scope").textContent).toBe("none")
  })

  it("setOpen(true) opens globally without a leftover folder scope", async () => {
    const user = userEvent.setup()
    render(
      <CreateRoomDialogProvider>
        <Probe />
      </CreateRoomDialogProvider>
    )

    await user.click(screen.getByRole("button", { name: "open folder" }))
    await user.click(screen.getByRole("button", { name: "close" }))
    await user.click(screen.getByRole("button", { name: "open global" }))

    expect(screen.getByTestId("open").textContent).toBe("true")
    expect(screen.getByTestId("scope").textContent).toBe("none")
  })
})

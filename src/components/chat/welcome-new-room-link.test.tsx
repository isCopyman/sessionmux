import { readFileSync } from "node:fs"
import { resolve } from "node:path"

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it } from "vitest"

import { WelcomeNewRoomLink } from "./welcome-hero"
import {
  CreateRoomDialogProvider,
  useCreateRoomDialog,
} from "@/contexts/create-room-dialog-context"
import enMessages from "@/i18n/messages/en.json"

/** Stands in for WorkspaceChromeController's mount of the real dialog. */
function DialogProbe() {
  const { open } = useCreateRoomDialog()
  return open ? <div>Create Room Dialog</div> : null
}

function renderEntry() {
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CreateRoomDialogProvider>
        <WelcomeNewRoomLink />
        <DialogProbe />
      </CreateRoomDialogProvider>
    </NextIntlClientProvider>
  )
  return userEvent.setup()
}

describe("WelcomeNewRoomLink", () => {
  it("offers the sidebar's create-room entry on the welcome page", () => {
    renderEntry()
    // Same label as the sidebar row (Folder.sidebar.newRoom) — one string, two
    // places, so the two entries can't drift apart.
    expect(
      screen.getByRole("button", { name: enMessages.Folder.sidebar.newRoom })
    ).toBeTruthy()
  })

  it("opens the shared create-room dialog instead of a private copy", async () => {
    const user = renderEntry()
    expect(screen.queryByText("Create Room Dialog")).toBeNull()

    await user.click(
      screen.getByRole("button", { name: enMessages.Folder.sidebar.newRoom })
    )

    expect(screen.getByText("Create Room Dialog")).toBeTruthy()
  })
})

describe("create-room dialog ownership", () => {
  const controllerSource = readFileSync(
    resolve(
      process.cwd(),
      "src/components/layout/workspace-chrome-controller.tsx"
    ),
    "utf8"
  )
  const sidebarSource = readFileSync(
    resolve(process.cwd(), "src/components/layout/sidebar.tsx"),
    "utf8"
  )

  // The sidebar unmounts when collapsed, so a dialog mounted there would make
  // the welcome-page entry a dead button. Exactly one always-mounted owner.
  it("mounts the dialog in the always-mounted controller, not the sidebar", () => {
    // Regex, not `toContain`: prettier breaks the tag across lines as soon as
    // another prop is added, and this test is about *where* the dialog is
    // mounted, not how the JSX happens to wrap today.
    expect(controllerSource).toMatch(/<CreateRoomDialog\s+open\b/)
    expect(controllerSource).toContain("useCreateRoomDialog()")
    expect(sidebarSource).not.toContain("<CreateRoomDialog")
    expect(sidebarSource).toContain("useCreateRoomDialog()")
  })
})

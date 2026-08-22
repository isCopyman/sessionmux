import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { MessageCommonActions } from "./message-common-actions"
import enMessages from "@/i18n/messages/en.json"
import { copyTextToClipboard } from "@/lib/utils"

vi.mock("@/lib/utils", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/utils")>()
  return { ...actual, copyTextToClipboard: vi.fn().mockResolvedValue(true) }
})

function renderActions() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <MessageCommonActions
        copyText="room answer"
        model="claude-opus-5"
        profile="cpa"
      />
    </NextIntlClientProvider>
  )
}

describe("MessageCommonActions", () => {
  beforeEach(() => vi.mocked(copyTextToClipboard).mockClear())

  it("shares copy, model and launch-profile controls across timelines", async () => {
    renderActions()

    fireEvent.click(screen.getByRole("button", { name: "Copy" }))
    await waitFor(() =>
      expect(copyTextToClipboard).toHaveBeenCalledWith("room answer")
    )
    expect(screen.getByRole("button", { name: "Model" })).toBeInTheDocument()
    expect(
      screen.getByRole("button", { name: "Launch profile" })
    ).toBeInTheDocument()
  })
})

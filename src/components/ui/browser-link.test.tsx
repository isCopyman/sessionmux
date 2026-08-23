import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("@/lib/platform", () => ({ openUrl: vi.fn() }))

import { BrowserLink } from "@/components/ui/browser-link"
import { openUrl } from "@/lib/platform"

describe("BrowserLink", () => {
  beforeEach(() => vi.clearAllMocks())

  it("keeps link semantics while routing the click through the platform opener", () => {
    render(<BrowserLink href="https://example.com">Docs</BrowserLink>)

    const link = screen.getByRole("link", { name: "Docs" })
    expect(link).toHaveAttribute("href", "https://example.com")
    expect(link).toHaveAttribute("target", "_blank")
    fireEvent.click(link)
    expect(openUrl).toHaveBeenCalledWith("https://example.com")
  })

  it("allows a caller to cancel opening", () => {
    render(
      <BrowserLink
        href="https://example.com"
        onClick={(event) => event.preventDefault()}
      >
        Docs
      </BrowserLink>
    )

    fireEvent.click(screen.getByRole("link", { name: "Docs" }))
    expect(openUrl).not.toHaveBeenCalled()
  })
})

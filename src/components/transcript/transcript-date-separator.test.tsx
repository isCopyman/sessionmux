import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { TranscriptDateSeparator } from "./transcript-date-separator"

describe("TranscriptDateSeparator", () => {
  it("renders the label as an accessible separator", () => {
    render(<TranscriptDateSeparator label="21 Aug 2026" />)
    const node = screen.getByRole("separator")
    expect(node).toHaveAttribute("data-transcript-date-separator")
    expect(node).toHaveTextContent("21 Aug 2026")
  })
})

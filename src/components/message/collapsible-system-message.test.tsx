import { type ReactElement } from "react"
import { fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { afterEach, describe, expect, it } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import type { AdaptedContentPart } from "@/lib/adapters/ai-elements-adapter"

import { CollapsibleSystemMessage } from "./collapsible-system-message"

const parts: AdaptedContentPart[] = [
  { type: "text", text: "Continuation summary remains visible" },
]

function renderWithIntl(ui: ReactElement) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {ui}
    </NextIntlClientProvider>
  )
}

function mockMetrics(scrollHeight: number, clientHeight: number) {
  const previous = (["scrollHeight", "clientHeight"] as const).map(
    (property) =>
      [
        property,
        Object.getOwnPropertyDescriptor(Element.prototype, property),
      ] as const
  )
  Object.defineProperty(Element.prototype, "scrollHeight", {
    configurable: true,
    get: () => scrollHeight,
  })
  Object.defineProperty(Element.prototype, "clientHeight", {
    configurable: true,
    get: () => clientHeight,
  })
  return () => {
    for (const [property, descriptor] of previous) {
      if (descriptor) {
        Object.defineProperty(Element.prototype, property, descriptor)
      }
    }
  }
}

describe("CollapsibleSystemMessage", () => {
  let restore: (() => void) | null = null

  afterEach(() => {
    restore?.()
    restore = null
  })

  it("shows a short summary immediately", () => {
    renderWithIntl(<CollapsibleSystemMessage parts={parts} />)
    expect(
      screen.getByText("Continuation summary remains visible")
    ).toBeVisible()
    expect(
      screen.queryByTestId("collapsible-system-message-toggle")
    ).not.toBeInTheDocument()
  })

  it("clamps long summaries and expands them on request", () => {
    restore = mockMetrics(900, 288)
    renderWithIntl(<CollapsibleSystemMessage parts={parts} />)

    const content = screen.getByTestId("collapsible-system-message-content")
    const toggle = screen.getByTestId("collapsible-system-message-toggle")
    expect(content).toHaveClass("max-h-72", "collapsed-content-fade")
    expect(toggle).toHaveAttribute("aria-expanded", "false")

    fireEvent.click(toggle)
    expect(toggle).toHaveAttribute("aria-expanded", "true")
    expect(content).not.toHaveClass("max-h-72")
  })
})

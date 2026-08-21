import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

const harness = vi.hoisted(() => ({
  configureLanguageValidation: vi.fn(),
  defineMonacoThemes: vi.fn(),
  getAction: vi.fn(),
  runFormat: vi.fn(),
}))

// Keep the same textarea stand-in used by the existing settings Monaco test,
// while also exercising beforeMount and onMount for this wrapper's contracts.
vi.mock("@monaco-editor/react", async () => {
  const { createElement, useEffect } = await import("react")
  const Editor = ({
    value,
    language,
    height,
    options,
    beforeMount,
    onMount,
    onChange,
  }: {
    value?: string
    language?: string
    height?: number | string
    options?: { ariaLabel?: string; tabSize?: number }
    beforeMount?: (monaco: object) => void
    onMount?: (editor: object, monaco: object) => void
    onChange?: (value: string | undefined) => void
  }) => {
    useEffect(() => {
      const monaco = {}
      beforeMount?.(monaco)
      onMount?.({ getAction: harness.getAction }, monaco)
    }, [beforeMount, onMount])
    return createElement("textarea", {
      "aria-label": options?.ariaLabel,
      "data-height": height,
      "data-language": language,
      "data-tab-size": options?.tabSize,
      value: value ?? "",
      onChange: (event: { target: { value: string } }) =>
        onChange?.(event.target.value),
    })
  }
  return { default: Editor }
})

vi.mock("@/hooks/use-appearance", () => ({
  useEditorFont: () => ({
    editorFontStack: "monospace",
    editorFontSize: 12,
  }),
}))

vi.mock("@/lib/monaco-themes", () => ({
  configureLanguageValidation: harness.configureLanguageValidation,
  defineMonacoThemes: harness.defineMonacoThemes,
  useMonacoThemeSync: () => "codeg-light-zinc",
}))

vi.mock("@/lib/monaco-local", () => ({}))

import enMessages from "@/i18n/messages/en.json"
import { JsonConfigEditor } from "./json-config-editor"

describe("JsonConfigEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    harness.getAction.mockReturnValue({ run: harness.runFormat })
  })

  it("keeps the controlled JSON contract and runs Monaco's formatter", async () => {
    const onChange = vi.fn()
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <JsonConfigEditor
          label="settings.json"
          value="{}"
          onChange={onChange}
          height={256}
        />
      </NextIntlClientProvider>
    )

    const editor = await screen.findByLabelText("settings.json")
    expect(editor).toHaveAttribute("data-language", "json")
    expect(editor).toHaveAttribute("data-height", "256")
    expect(editor).toHaveAttribute("data-tab-size", "2")

    fireEvent.change(editor, { target: { value: '{"env":{}}' } })
    expect(onChange).toHaveBeenCalledWith('{"env":{}}')

    await waitFor(() =>
      expect(harness.defineMonacoThemes).toHaveBeenCalledOnce()
    )
    fireEvent.click(screen.getByRole("button", { name: "Format" }))
    expect(harness.getAction).toHaveBeenCalledWith(
      "editor.action.formatDocument"
    )
    expect(harness.runFormat).toHaveBeenCalledOnce()
    expect(harness.configureLanguageValidation).toHaveBeenCalledOnce()
  })
})

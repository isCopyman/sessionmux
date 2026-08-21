"use client"

import { useRef } from "react"
import dynamic from "next/dynamic"
import type { BeforeMount, OnMount } from "@monaco-editor/react"
import { Loader2 } from "lucide-react"
import { useTranslations } from "next-intl"

import { Button } from "@/components/ui/button"
import { useEditorFont } from "@/hooks/use-appearance"
import {
  configureLanguageValidation,
  defineMonacoThemes,
  useMonacoThemeSync,
} from "@/lib/monaco-themes"
import "@/lib/monaco-local"

const MonacoEditor = dynamic(async () => import("@monaco-editor/react"), {
  ssr: false,
})

const prepareMonaco: BeforeMount = (monaco) => {
  defineMonacoThemes(monaco)
  configureLanguageValidation(monaco)
}

interface JsonConfigEditorProps {
  label: string
  value: string
  onChange: (value: string) => void
  /** Passed straight to Monaco. A `vh` string lets the editor grow with the
   *  window, which is what a settings.json of unknown length needs. */
  height: number | string
}

export function JsonConfigEditor({
  label,
  value,
  onChange,
  height,
}: JsonConfigEditorProps) {
  const t = useTranslations("AcpAgentSettings.claudeProfile")
  const { editorFontStack, editorFontSize } = useEditorFont()
  const monacoTheme = useMonacoThemeSync()
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null)

  const handleMount: OnMount = (editor) => {
    editorRef.current = editor
  }

  const formatDocument = () => {
    void editorRef.current?.getAction("editor.action.formatDocument")?.run()
  }

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-muted-foreground">{label}</span>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          onClick={formatDocument}
          title={t("formatJson")}
        >
          {t("formatJson")}
        </Button>
      </div>
      <div className="overflow-hidden rounded-md border">
        <MonacoEditor
          height={height}
          language="json"
          theme={monacoTheme}
          beforeMount={prepareMonaco}
          onMount={handleMount}
          value={value}
          onChange={(nextValue) => onChange(nextValue ?? "")}
          loading={
            <div className="flex h-full items-center justify-center">
              <Loader2 className="size-4 animate-spin text-muted-foreground" />
            </div>
          }
          options={{
            ariaLabel: label,
            fontFamily: editorFontStack,
            fontSize: editorFontSize,
            minimap: { enabled: false },
            wordWrap: "on",
            scrollBeyondLastLine: false,
            lineNumbers: "on",
            tabSize: 2,
            automaticLayout: true,
            renderWhitespace: "selection",
          }}
        />
      </div>
    </div>
  )
}

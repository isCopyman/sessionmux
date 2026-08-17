"use client"

import { useMemo } from "react"
import { useTranslations } from "next-intl"

import { CollapsibleUserMessage } from "./collapsible-user-message"

export function SessionLetterBody({
  subject,
  body,
}: {
  subject?: string | null
  body: string
}) {
  const t = useTranslations("Collaboration")
  const title = subject?.trim() ?? ""
  const parts = useMemo(() => [{ type: "text" as const, text: body }], [body])
  return (
    <div className="min-w-0" data-session-letter="">
      {title ? (
        <div className="mb-1.5">
          <p className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {t("letterSubject")}
          </p>
          <p
            data-collaboration-subject=""
            className="text-sm font-semibold leading-snug tracking-tight"
          >
            {title}
          </p>
        </div>
      ) : null}
      {body ? (
        <div
          className={
            title
              ? "rounded-md bg-muted/45 px-2.5 py-2 leading-relaxed"
              : undefined
          }
        >
          <CollapsibleUserMessage parts={parts} />
        </div>
      ) : null}
    </div>
  )
}

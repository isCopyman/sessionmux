"use client"

import { memo } from "react"
import { useTranslations } from "next-intl"
import { ChevronDown, ChevronUp, Info } from "lucide-react"

import { useCollapsibleOverflow } from "@/hooks/use-collapsible-overflow"
import type { AdaptedContentPart } from "@/lib/adapters/ai-elements-adapter"
import { cn } from "@/lib/utils"

import { ContentPartsRenderer } from "./content-parts-renderer"

/**
 * System messages commonly contain the continuation summary created after a
 * context compaction. Keep the first screenful visible so that useful context
 * is not hidden behind a closed accordion, while still clamping long summaries.
 */
export const CollapsibleSystemMessage = memo(function CollapsibleSystemMessage({
  parts,
}: {
  parts: AdaptedContentPart[]
}) {
  const t = useTranslations("Folder.chat.messageList")
  const { contentRef, contentId, isOverflowing, expanded, toggle } =
    useCollapsibleOverflow<HTMLDivElement>(parts)
  const clipped = !expanded

  return (
    <div className="w-full overflow-hidden rounded-md border border-yellow-500/30 bg-yellow-500/5 text-sm">
      <div className="flex items-center gap-1.5 border-b border-yellow-500/20 bg-yellow-500/10 px-3 py-2 text-xs font-medium text-yellow-700 dark:text-yellow-400">
        <Info className="size-3.5 shrink-0" />
        {t("systemMessage")}
      </div>
      <div
        ref={contentRef}
        id={contentId}
        data-testid="collapsible-system-message-content"
        className={cn(
          "px-3 py-2.5 text-sm text-muted-foreground",
          clipped && "max-h-72 overflow-hidden",
          clipped && isOverflowing && "collapsed-content-fade"
        )}
      >
        <ContentPartsRenderer parts={parts} role="system" />
      </div>
      {isOverflowing && (
        <button
          type="button"
          data-testid="collapsible-system-message-toggle"
          onClick={toggle}
          aria-expanded={expanded}
          aria-controls={contentId}
          className="flex w-full items-center justify-center gap-1 border-t border-yellow-500/20 px-3 py-1.5 text-xs font-medium text-yellow-700/90 transition-colors hover:bg-yellow-500/10 dark:text-yellow-400/90"
        >
          {expanded ? t("showLess") : t("showMore")}
          {expanded ? (
            <ChevronUp className="size-3.5 shrink-0" />
          ) : (
            <ChevronDown className="size-3.5 shrink-0" />
          )}
        </button>
      )}
    </div>
  )
})

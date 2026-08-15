"use client"

import { useEffect, useRef, type KeyboardEvent } from "react"
import { ChevronDown, ChevronUp, Loader2, Search, X } from "lucide-react"
import { useTranslations } from "next-intl"
import { Button } from "@/components/ui/button"

interface ConversationFindBarProps {
  query: string
  current: number
  total: number
  searching: boolean
  focusToken: number
  onQueryChange: (query: string) => void
  onNext: () => void
  onPrevious: () => void
  onClose: () => void
}

export function ConversationFindBar({
  query,
  current,
  total,
  searching,
  focusToken,
  onQueryChange,
  onNext,
  onPrevious,
  onClose,
}: ConversationFindBarProps) {
  const t = useTranslations("Folder.chat.messageList")
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus({ preventScroll: true })
    inputRef.current?.select()
  }, [focusToken])

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault()
      onClose()
      return
    }
    if (event.key !== "Enter" || total === 0) return
    event.preventDefault()
    if (event.shiftKey) onPrevious()
    else onNext()
  }

  const resultText =
    query.length === 0
      ? "0 / 0"
      : total > 0
        ? `${current} / ${total}`
        : searching
          ? t("findSearching")
          : t("findNoMatches")

  return (
    <div
      data-conversation-find-bar
      role="search"
      className="absolute end-3 top-3 z-40 flex h-9 w-[min(28rem,calc(100%-1.5rem))] items-center gap-1 rounded-md border border-border bg-background/98 px-1.5 shadow-lg backdrop-blur-sm"
    >
      <Search
        aria-hidden="true"
        className="size-3.5 shrink-0 text-muted-foreground"
      />
      <input
        ref={inputRef}
        data-conversation-find-input
        value={query}
        onChange={(event) => onQueryChange(event.target.value)}
        onKeyDown={handleKeyDown}
        aria-label={t("findInConversation")}
        placeholder={t("findPlaceholder")}
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        className="h-7 min-w-0 flex-1 bg-transparent px-1 text-sm outline-none placeholder:text-muted-foreground"
      />
      <span
        aria-live="polite"
        className="flex min-w-[4.5rem] shrink-0 items-center justify-end gap-1 whitespace-nowrap text-[0.6875rem] tabular-nums text-muted-foreground"
      >
        {searching && query.length > 0 ? (
          <Loader2 aria-hidden="true" className="size-3 animate-spin" />
        ) : null}
        {resultText}
      </span>
      <Button
        type="button"
        size="icon-xs"
        variant="ghost"
        disabled={total === 0}
        aria-label={t("findPrevious")}
        onClick={onPrevious}
      >
        <ChevronUp aria-hidden="true" className="size-3.5" />
      </Button>
      <Button
        type="button"
        size="icon-xs"
        variant="ghost"
        disabled={total === 0}
        aria-label={t("findNext")}
        onClick={onNext}
      >
        <ChevronDown aria-hidden="true" className="size-3.5" />
      </Button>
      <Button
        type="button"
        size="icon-xs"
        variant="ghost"
        aria-label={t("findClose")}
        onClick={onClose}
      >
        <X aria-hidden="true" className="size-3.5" />
      </Button>
    </div>
  )
}

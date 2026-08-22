"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { BrainCog, CheckIcon, CopyIcon, IdCard, ListTodo } from "lucide-react"
import { useTranslations } from "next-intl"

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn, copyTextToClipboard } from "@/lib/utils"

export const messageActionButtonClass =
  "inline-flex h-6 w-6 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"

export interface MessageCommonActionsProps {
  copyText?: string | null
  model?: string | null
  models?: string[]
  profile?: string | null
  onCreateTask?: (() => void) | null
}

/**
 * Actions and immutable authoring metadata shared by Session replies and Room
 * posts. Room-specific reply/mention controls deliberately stay in the Room
 * renderer; copy, model and launch profile must not drift between timelines.
 */
export function MessageCommonActions({
  copyText = "",
  model,
  models,
  profile,
  onCreateTask,
}: MessageCommonActionsProps) {
  const t = useTranslations("Folder.chat.messageList")
  const tTasks = useTranslations("Tasks")
  const tProfile = useTranslations("AcpAgentSettings.claudeProfile")
  const [isCopied, setIsCopied] = useState(false)
  const timeoutRef = useRef<number>(0)
  const normalizedCopyText = copyText ?? ""
  const hasCopy = normalizedCopyText.trim().length > 0
  const displayModels = models?.length ? models : model ? [model] : []

  const handleCopy = useCallback(async () => {
    if (isCopied || !hasCopy) return
    window.clearTimeout(timeoutRef.current)
    const ok = await copyTextToClipboard(normalizedCopyText)
    if (!ok) return
    setIsCopied(true)
    timeoutRef.current = window.setTimeout(() => setIsCopied(false), 2000)
  }, [hasCopy, isCopied, normalizedCopyText])

  useEffect(
    () => () => {
      window.clearTimeout(timeoutRef.current)
    },
    []
  )

  if (!hasCopy && displayModels.length === 0 && !profile) return null

  return (
    <TooltipProvider delayDuration={150}>
      {hasCopy ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={handleCopy}
              className={messageActionButtonClass}
              aria-label={isCopied ? t("copied") : t("copyMessage")}
            >
              {isCopied ? (
                <CheckIcon aria-hidden="true" className="h-3.5 w-3.5" />
              ) : (
                <CopyIcon aria-hidden="true" className="h-3.5 w-3.5" />
              )}
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">
            {isCopied ? t("copied") : t("copyMessage")}
          </TooltipContent>
        </Tooltip>
      ) : null}
      {hasCopy && onCreateTask ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={onCreateTask}
              className={messageActionButtonClass}
              aria-label={tTasks("createFromMessage")}
            >
              <ListTodo aria-hidden="true" className="h-3.5 w-3.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">
            {tTasks("createFromMessage")}
          </TooltipContent>
        </Tooltip>
      ) : null}
      {displayModels.length > 0 ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className={cn(messageActionButtonClass, "cursor-default")}
              aria-label={t("model")}
            >
              <BrainCog aria-hidden="true" className="h-3.5 w-3.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top" className="max-w-xs break-words">
            <span className="font-medium" translate="no">
              {displayModels.join(", ")}
            </span>
          </TooltipContent>
        </Tooltip>
      ) : null}
      {profile ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className={cn(messageActionButtonClass, "cursor-default")}
              aria-label={tProfile("controlName")}
            >
              <IdCard aria-hidden="true" className="h-3.5 w-3.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top" className="max-w-xs break-words">
            <span className="font-medium" translate="no">
              {profile}
            </span>
          </TooltipContent>
        </Tooltip>
      ) : null}
    </TooltipProvider>
  )
}

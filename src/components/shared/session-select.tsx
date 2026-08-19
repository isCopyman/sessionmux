"use client"

import { memo, useState } from "react"
import { Check, ChevronDown, MessageSquare } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { getAgentLabel } from "@/lib/custom-agents"
import type { AgentType } from "@/lib/types"
import { cn } from "@/lib/utils"

/** The minimum a session has to carry to be listed. Modeled on
 *  `FolderSelectOption` — a caller can offer a session it only knows by id
 *  (an automation whose target was deleted, say). */
export interface SessionSelectOption {
  id: number
  title: string | null
  agent_type?: AgentType
}

/** The string cmdk matches the query against: the title, the `#id` handle,
 *  and the agent label — everything a user might type to find a session. */
function sessionSearchValue(s: SessionSelectOption): string {
  return `${s.title ?? ""} #${s.id} ${s.agent_type ?? ""}`
}

/** One session row: title (or `#id`) over its agent, trailing check for the
 *  current one. Mirrors `FolderOptionItem`. */
export const SessionOptionItem = memo(function SessionOptionItem({
  session,
  selected,
  onSelect,
}: {
  session: SessionSelectOption
  selected: boolean
  onSelect: () => void
}) {
  return (
    <CommandItem value={sessionSearchValue(session)} onSelect={onSelect}>
      <MessageSquare className="h-4 w-4" />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate font-medium">
          {session.title ?? `#${session.id}`}
        </span>
        {session.agent_type ? (
          <span className="truncate text-start text-xs text-muted-foreground">
            {getAgentLabel(session.agent_type) ?? session.agent_type}
          </span>
        ) : null}
      </div>
      {selected ? <Check className="h-4 w-4 shrink-0" /> : null}
    </CommandItem>
  )
})

interface SessionSelectProps {
  sessions: readonly SessionSelectOption[]
  /** The chosen session id, or `null` for "no choice yet". */
  value: number | null
  onChange: (sessionId: number) => void
  /** Trigger text while nothing is selected. */
  placeholder?: string
  searchPlaceholder?: string
  emptyLabel?: string
  disabled?: boolean
  /** Prefixes the trigger tooltip, e.g. "Target session". */
  title?: string
  className?: string
}

/**
 * A searchable session picker: a compact trigger that opens a command-palette
 * list (search box, one row per session showing title over its agent,
 * trailing check). The Automation editor uses it to pick the Session a
 * queue-prompt automation enqueues into. Mirrors `FolderSelect`'s "field"
 * look; the labels come in as props so the component stays namespace-free.
 */
export function SessionSelect({
  sessions,
  value,
  onChange,
  placeholder,
  searchPlaceholder,
  emptyLabel,
  disabled = false,
  title,
  className,
}: SessionSelectProps) {
  const [open, setOpen] = useState(false)

  const current =
    value != null ? sessions.find((s) => s.id === value) : undefined
  // A selection the list can no longer resolve — the session was deleted or
  // archived while it was the chosen one. It must NOT read as the null state:
  // the saved automation still targets that id, so show the `#<id>` handle
  // (the same idiom FolderSelect uses for a folder it can only name that way).
  const unresolved = value != null && !current
  const currentText = current ? (current.title ?? `#${current.id}`) : null
  const fallbackText = unresolved ? `#${value}` : (placeholder ?? "")
  const tooltip = [title, currentText ?? fallbackText]
    .filter(Boolean)
    .join(" · ")

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        if (!disabled) setOpen(o)
      }}
    >
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={disabled}
          title={tooltip}
          className={cn(
            "h-7 w-auto min-w-0 max-w-[16rem] gap-1.5 rounded-4xl border-input bg-input/30 px-3 text-xs font-normal hover:bg-input/50 dark:hover:bg-input/50",
            className
          )}
        >
          <MessageSquare
            className="size-3.5 shrink-0 text-muted-foreground"
            aria-hidden="true"
          />
          <span
            className={cn(
              "min-w-0 flex-1 truncate text-start",
              !current && (placeholder || unresolved) && "text-muted-foreground"
            )}
          >
            {currentText ?? fallbackText}
          </span>
          <ChevronDown
            className="size-3.5 shrink-0 text-muted-foreground/60"
            aria-hidden="true"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72 overflow-hidden p-0">
        <Command className="rounded-2xl">
          <CommandInput placeholder={searchPlaceholder} />
          <CommandList>
            <CommandEmpty>{emptyLabel}</CommandEmpty>
            <CommandGroup>
              {sessions.map((s) => (
                <SessionOptionItem
                  key={s.id}
                  session={s}
                  selected={s.id === value}
                  onSelect={() => {
                    setOpen(false)
                    onChange(s.id)
                  }}
                />
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

"use client"

import { useMemo, useState } from "react"
import { useTranslations } from "next-intl"

import { AgentIcon } from "@/components/agent-icon"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { formatRelative } from "@/components/conversations/sidebar-conversation-grouping"
import { formatConversationTitle } from "@/lib/conversation-title"
import { roomMemberCandidates } from "@/lib/room-create"
import type { DbConversationSummary, FolderDetail } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"

type FolderLabel = Pick<FolderDetail, "id" | "name" | "alias">

type SessionPickerBase = {
  candidates: DbConversationSummary[]
  query: string
  onQueryChange: (query: string) => void
  emptyLabel: string
  excludeIds?: readonly number[]
}

export type SessionPickerProps =
  | (SessionPickerBase & {
      mode: "single"
      value: number | null
      onChange: (id: number | null) => void
    })
  | (SessionPickerBase & {
      mode: "multi"
      value: number[]
      onChange: (ids: number[]) => void
    })

function folderDisplayName(
  folderId: number,
  folders: readonly FolderLabel[],
  fallback: string
): string {
  const folder = folders.find((item) => item.id === folderId)
  if (folder == null) return fallback
  return folder.alias ?? folder.name
}

function SessionMeta({
  conversation,
  folders,
  untitled,
  now,
}: {
  conversation: DbConversationSummary
  folders: readonly FolderLabel[]
  untitled: (id: number) => string
  now: number
}) {
  const t = useTranslations("Room")
  const title =
    formatConversationTitle(conversation.title) || untitled(conversation.id)
  const relative = formatRelative(conversation.updated_at ?? "", now)
  const folderName = folderDisplayName(
    conversation.folder_id,
    folders,
    t("uncategorized")
  )
  return (
    <>
      <AgentIcon
        agentType={conversation.agent_type}
        className="mt-0.5 h-3.5 w-3.5"
      />
      <span className="min-w-0 flex-1">
        <span className="block truncate">{title}</span>
        <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
          {relative ? `${folderName} · ${relative}` : folderName}
        </span>
      </span>
    </>
  )
}

const ROW_CLASS =
  "flex w-full cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 text-left text-xs font-normal hover:bg-muted/60"

/**
 * Shared Session list for "pick an initiator" (single) and "add members"
 * (multi). Rows are Label-wrapped so Radix Checkbox/Radio never nest inside a
 * <button> — that nesting was the Rooms add-member hydration error.
 */
export function SessionPicker(props: SessionPickerProps) {
  const t = useTranslations("Room")
  const folders = useAppWorkspaceStore((state) => state.folders) ?? []
  const [now] = useState(() => Date.now())
  const visible = useMemo(() => {
    const exclude = new Set(props.excludeIds ?? [])
    return roomMemberCandidates(props.candidates, props.query).filter(
      (conversation) => !exclude.has(conversation.id)
    )
  }, [props.candidates, props.excludeIds, props.query])
  const untitled = (id: number) => t("untitled", { id })

  return (
    <>
      <Input
        value={props.query}
        onChange={(event) => props.onQueryChange(event.target.value)}
        placeholder={t("addMemberSearch")}
      />
      <ScrollArea className="h-56">
        {visible.length === 0 ? (
          <p className="px-1 py-6 text-center text-xs text-muted-foreground">
            {props.emptyLabel}
          </p>
        ) : props.mode === "single" ? (
          <RadioGroup
            value={props.value != null ? String(props.value) : ""}
            onValueChange={(value) => props.onChange(Number(value))}
            className="flex flex-col gap-1"
          >
            {visible.map((conversation) => (
              <Label key={conversation.id} className={ROW_CLASS}>
                <RadioGroupItem
                  value={String(conversation.id)}
                  className="mt-0.5"
                />
                <SessionMeta
                  conversation={conversation}
                  folders={folders}
                  untitled={untitled}
                  now={now}
                />
              </Label>
            ))}
          </RadioGroup>
        ) : (
          <ul className="flex flex-col gap-1">
            {visible.map((conversation) => {
              const checked = props.value.includes(conversation.id)
              return (
                <li key={conversation.id}>
                  <Label className={ROW_CLASS}>
                    <Checkbox
                      checked={checked}
                      className="mt-0.5"
                      onCheckedChange={(next) => {
                        const isOn = next === true
                        if (isOn) {
                          if (!props.value.includes(conversation.id)) {
                            props.onChange([...props.value, conversation.id])
                          }
                          return
                        }
                        props.onChange(
                          props.value.filter((id) => id !== conversation.id)
                        )
                      }}
                    />
                    <SessionMeta
                      conversation={conversation}
                      folders={folders}
                      untitled={untitled}
                      now={now}
                    />
                  </Label>
                </li>
              )
            })}
          </ul>
        )}
      </ScrollArea>
    </>
  )
}

"use client"

import { forwardRef, type ButtonHTMLAttributes } from "react"
import { ListTodo, Users } from "lucide-react"

import { AgentIcon } from "@/components/agent-icon"
import { CollaborationUnreadBadge } from "@/components/collaboration/collaboration-unread-badge"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import type { AgentType, ConversationStatus } from "@/lib/types"
import type { TaskBoardScope } from "@/lib/task-board-scope"
import { cn } from "@/lib/utils"

export type WorkbenchContentKind = "conversation" | "room" | "board"
export type WorkbenchContentActivity = "busy" | "attention" | null

interface WorkbenchContentRowProps extends Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "children" | "onClick" | "title"
> {
  kind: WorkbenchContentKind
  title: string
  agentType: AgentType
  selected: boolean
  workbenchId: number
  conversationId?: number | null
  roomId?: string
  boardScope?: TaskBoardScope
  status?: ConversationStatus
  activity?: WorkbenchContentActivity
  unreadCount?: number
  workingLabel: string
  attentionLabel: string
  onClick: () => void
}

/**
 * Shared Workbench navigation row for every content tab. It owns only the
 * geometry, focus treatment and common status projection. Session, Room and
 * Board actions remain outside this component so a capability that makes no
 * sense for one kind cannot leak into its menu.
 */
export const WorkbenchContentRow = forwardRef<
  HTMLButtonElement,
  WorkbenchContentRowProps
>(function WorkbenchContentRow(
  {
    kind,
    title,
    agentType,
    selected,
    workbenchId,
    conversationId,
    roomId,
    boardScope,
    status,
    activity = null,
    unreadCount = 0,
    workingLabel,
    attentionLabel,
    onClick,
    ...triggerProps
  },
  ref
) {
  return (
    <button
      {...triggerProps}
      ref={ref}
      type="button"
      data-workbench-id={workbenchId}
      data-workbench-session
      data-focused-session={selected ? "true" : undefined}
      data-conversation-id={conversationId ?? undefined}
      data-room-id={roomId}
      data-board-scope={boardScope}
      title={title}
      aria-current={selected ? "page" : undefined}
      className={cn(
        "flex h-7 w-full min-w-0 items-center gap-1.5 rounded-md pe-2 ps-8 text-start text-xs",
        "hover:bg-sidebar-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
        selected &&
          "bg-primary/8 text-primary ring-1 ring-inset ring-primary/30"
      )}
      onClick={onClick}
    >
      <span
        aria-hidden
        className="relative flex h-3.5 w-3.5 shrink-0 items-center justify-center"
      >
        {kind === "room" ? (
          <Users className="h-3 w-3" />
        ) : kind === "board" ? (
          <ListTodo className="h-3 w-3" />
        ) : (
          <AgentIcon agentType={agentType} className="h-3 w-3" />
        )}
        {activity === "busy" ? (
          <span
            title={workingLabel}
            className={cn(
              "absolute -top-0.5 -left-0.5 h-1.5 w-1.5 rounded-full",
              "bg-primary animate-pulse ring-1 ring-sidebar"
            )}
          />
        ) : activity === "attention" ? (
          <span
            title={attentionLabel}
            className={cn(
              "absolute -top-0.5 -left-0.5 h-1.5 w-1.5 rounded-full",
              "bg-destructive ring-1 ring-sidebar"
            )}
          />
        ) : null}
        {status ? (
          <ConversationStatusDot
            status={status}
            size="sm"
            className="absolute -bottom-0.5 -right-0.5 ring-1 ring-sidebar"
          />
        ) : null}
      </span>
      <span className="min-w-0 flex-1 truncate">{title}</span>
      {kind === "room" ? (
        <CollaborationUnreadBadge count={unreadCount} className="ms-auto" />
      ) : null}
    </button>
  )
})

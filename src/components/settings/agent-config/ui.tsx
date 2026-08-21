"use client"

import { useCallback, type PointerEvent, type ReactNode } from "react"
import { Reorder, useDragControls } from "motion/react"
import { useTranslations } from "next-intl"
import { cn } from "@/lib/utils"
import type { AcpAgentInfo, AgentType } from "@/lib/types"
import { NATIVE_CONFIG_PATHS } from "./shared"

/**
 * Grey caption under a native-JSON editor naming the file it edits. The longer
 * story (merge-write semantics, the shortcut fields being a view of the same
 * file) rides along as a tooltip instead of another wall of text.
 */
export function NativeConfigFileHint({ agentType }: { agentType: AgentType }) {
  const t = useTranslations("AcpAgentSettings")
  const path = NATIVE_CONFIG_PATHS[agentType]
  return (
    <p
      className="text-[11px] text-muted-foreground"
      title={t("nativeJson.hint")}
    >
      {path
        ? t("nativeJson.editsPath", { path })
        : t("nativeJson.editsUnknownPath")}
    </p>
  )
}

export interface AgentReorderItemProps {
  agent: AcpAgentInfo
  selected: boolean
  reordering: boolean
  dragging: AgentType | null
  onDragStart: (agentType: AgentType) => void
  onDragEnd: () => void
  onSelect: (agentType: AgentType) => void
  children: (
    startDrag: (event: PointerEvent<HTMLButtonElement>) => void
  ) => ReactNode
}

export function AgentReorderItem({
  agent,
  selected,
  reordering,
  dragging,
  onDragStart,
  onDragEnd,
  onSelect,
  children,
}: AgentReorderItemProps) {
  const dragControls = useDragControls()

  const startDrag = useCallback(
    (event: PointerEvent<HTMLButtonElement>) => {
      event.preventDefault()
      event.stopPropagation()
      dragControls.start(event)
    },
    [dragControls]
  )

  return (
    <Reorder.Item
      as="section"
      value={agent}
      data-agent-type={agent.agent_type}
      drag={reordering ? false : "y"}
      dragListener={false}
      dragControls={dragControls}
      dragMomentum={false}
      layout="position"
      className={cn(
        "rounded-lg border bg-card p-3 transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40",
        selected && "border-primary/60 bg-primary/5",
        dragging === agent.agent_type && "border-primary/60 bg-primary/5"
      )}
      tabIndex={0}
      onDragStart={() => {
        onDragStart(agent.agent_type)
      }}
      onDragEnd={onDragEnd}
      onClick={() => {
        onSelect(agent.agent_type)
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.key !== "Enter" && event.key !== " ") return
        event.preventDefault()
        onSelect(agent.agent_type)
      }}
    >
      {children(startDrag)}
    </Reorder.Item>
  )
}

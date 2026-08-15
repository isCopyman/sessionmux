"use client"

import { useEffect, useRef, useState } from "react"
import { Reorder } from "motion/react"
import {
  ArrowLeft,
  ArrowRight,
  Copy,
  Loader2,
  PanelsTopLeft,
  Pin,
  PinOff,
  Plus,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { useTabStore } from "@/contexts/tab-context"
import { useWorkbenchStore } from "@/stores/workbench-store"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import type { WorkbenchInfo } from "@/lib/types"

interface WorkbenchTabStripProps {
  leftInset: number
  rightInset: number
}

/**
 * Window-level tabs for named workbenches. Session tabs live one row below,
 * inside their panes; this strip switches the complete saved work surface.
 * The empty tail remains a Tauri drag region, while the insets keep tabs clear
 * of the fixed window-chrome controls.
 */
export function WorkbenchTabStrip({
  leftInset,
  rightInset,
}: WorkbenchTabStripProps) {
  const t = useTranslations("Folder.workbench")
  const tConversation = useTranslations("Folder.conversationCard")
  const items = useWorkbenchStore((state) => state.items)
  const hydrated = useWorkbenchStore((state) => state.hydrated)
  const loading = useWorkbenchStore((state) => state.loading)
  const hydrate = useWorkbenchStore((state) => state.hydrate)
  const createAndSwitch = useWorkbenchStore((state) => state.createAndSwitch)
  const duplicateAndSwitch = useWorkbenchStore(
    (state) => state.duplicateAndSwitch
  )
  const previewOrder = useWorkbenchStore((state) => state.previewOrder)
  const persistOrder = useWorkbenchStore((state) => state.persistOrder)
  const setPinned = useWorkbenchStore((state) => state.setPinned)
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const switchingWorkbenchId = useTabStore(
    (state) => state.switchingWorkbenchId
  )
  const switchWorkbench = useTabStore((state) => state.switchWorkbench)
  const [creating, setCreating] = useState(false)
  const [copyingId, setCopyingId] = useState<number | null>(null)
  const [ordering, setOrdering] = useState(false)
  const [pinningId, setPinningId] = useState<number | null>(null)
  const dragStartOrderRef = useRef<string | null>(null)
  const dragClickGuardRef = useRef<number | null>(null)
  const busy =
    switchingWorkbenchId != null ||
    creating ||
    copyingId != null ||
    pinningId != null ||
    ordering

  useEffect(() => {
    if (hydrated) return
    void hydrate().catch((error) =>
      toast.error(t("loadFailed", { message: toErrorMessage(error) }))
    )
  }, [hydrate, hydrated, t])

  const create = async () => {
    if (busy) return
    setCreating(true)
    try {
      await createAndSwitch(t("defaultName", { number: items.length + 1 }))
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setCreating(false)
    }
  }

  const duplicate = async (item: WorkbenchInfo) => {
    if (busy) return
    setCopyingId(item.id)
    try {
      await duplicateAndSwitch(item.id, t("copyName", { name: item.name }))
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setCopyingId(null)
    }
  }

  const commitOrder = async () => {
    if (ordering) return
    setOrdering(true)
    try {
      await persistOrder()
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setOrdering(false)
    }
  }

  const moveBy = (item: WorkbenchInfo, delta: -1 | 1) => {
    if (busy) return
    const index = items.findIndex((candidate) => candidate.id === item.id)
    const target = index + delta
    if (index < 0 || target < 0 || target >= items.length) return
    if (items[target].is_pinned !== item.is_pinned) return
    const next = [...items]
    const current = next[index]
    next[index] = next[target]
    next[target] = current
    previewOrder(next)
    void commitOrder()
  }

  const togglePinned = async (item: WorkbenchInfo) => {
    if (busy) return
    setPinningId(item.id)
    try {
      await setPinned(item.id, !item.is_pinned)
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setPinningId(null)
    }
  }

  return (
    <div
      data-workbench-tab-strip
      className="flex h-10 shrink-0 items-stretch border-b border-border/60 bg-muted/70 ws-chrome-border ws-transparent-bg"
    >
      {leftInset > 0 && (
        <div
          data-tauri-drag-region
          className="h-full shrink-0"
          style={{ width: leftInset }}
        />
      )}
      <Reorder.Group
        as="div"
        role="tablist"
        aria-label={t("switchTitle")}
        axis="x"
        values={items.map((item) => item.id)}
        onReorder={(orderedIds) => {
          const byId = new Map(items.map((item) => [item.id, item]))
          previewOrder(
            orderedIds.flatMap((id) => {
              const item = byId.get(id)
              return item ? [item] : []
            })
          )
        }}
        className="flex min-w-0 items-end gap-0.5 overflow-x-auto px-1 pt-1"
      >
        {items.map((item, index) => {
          const active = item.id === activeWorkbenchId
          const restoring = item.id === switchingWorkbenchId
          const canMoveLeft =
            index > 0 && items[index - 1].is_pinned === item.is_pinned
          const canMoveRight =
            index < items.length - 1 &&
            items[index + 1].is_pinned === item.is_pinned
          return (
            <ContextMenu key={item.id}>
              <ContextMenuTrigger asChild>
                <Reorder.Item
                  as="div"
                  value={item.id}
                  drag="x"
                  dragListener={!busy}
                  onDragStart={() => {
                    dragClickGuardRef.current = item.id
                    dragStartOrderRef.current = items
                      .map((candidate) => candidate.id)
                      .join(",")
                  }}
                  onDragEnd={() => {
                    window.setTimeout(() => {
                      if (dragClickGuardRef.current === item.id) {
                        dragClickGuardRef.current = null
                      }
                    }, 0)
                    const currentOrder = useWorkbenchStore
                      .getState()
                      .items.map((candidate) => candidate.id)
                      .join(",")
                    if (currentOrder === dragStartOrderRef.current) return
                    void commitOrder()
                  }}
                  className="shrink-0"
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={active}
                    data-workbench-tab-id={item.id}
                    disabled={switchingWorkbenchId != null}
                    onClick={() => {
                      if (dragClickGuardRef.current === item.id) {
                        dragClickGuardRef.current = null
                        return
                      }
                      if (active) return
                      void switchWorkbench(item.id).catch((error) =>
                        toast.error(
                          t("switchFailed", {
                            message: toErrorMessage(error),
                          })
                        )
                      )
                    }}
                    onKeyDown={(event) => {
                      if (!event.altKey) return
                      if (event.key === "ArrowLeft") {
                        event.preventDefault()
                        moveBy(item, -1)
                      } else if (event.key === "ArrowRight") {
                        event.preventDefault()
                        moveBy(item, 1)
                      }
                    }}
                    className={cn(
                      "group flex h-9 max-w-52 min-w-24 items-center gap-2 rounded-t-md border border-transparent px-3 text-xs font-medium outline-none transition-colors",
                      "hover:bg-background/55 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                      active &&
                        "border-border/70 border-b-background bg-background text-foreground",
                      !active && "text-muted-foreground",
                      busy && "opacity-70"
                    )}
                    title={item.name}
                  >
                    {restoring ||
                    copyingId === item.id ||
                    pinningId === item.id ? (
                      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
                    ) : (
                      <PanelsTopLeft
                        className={cn(
                          "h-3.5 w-3.5 shrink-0",
                          active && "text-primary"
                        )}
                      />
                    )}
                    {item.is_pinned ? (
                      <Pin
                        aria-hidden
                        className="h-3 w-3 shrink-0 text-primary/75"
                      />
                    ) : null}
                    <span className="truncate">{item.name}</span>
                  </button>
                </Reorder.Item>
              </ContextMenuTrigger>
              <ContextMenuContent>
                <ContextMenuItem
                  disabled={busy}
                  onSelect={() => void togglePinned(item)}
                >
                  {item.is_pinned ? (
                    <PinOff className="h-4 w-4" />
                  ) : (
                    <Pin className="h-4 w-4" />
                  )}
                  {item.is_pinned
                    ? tConversation("unpin")
                    : tConversation("pin")}
                </ContextMenuItem>
                <ContextMenuItem
                  disabled={busy}
                  onSelect={() => void duplicate(item)}
                >
                  <Copy className="h-4 w-4" />
                  {t("duplicate")}
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem
                  disabled={busy || !canMoveLeft}
                  onSelect={() => moveBy(item, -1)}
                >
                  <ArrowLeft className="h-4 w-4" />
                  {t("moveLeft")}
                </ContextMenuItem>
                <ContextMenuItem
                  disabled={busy || !canMoveRight}
                  onSelect={() => moveBy(item, 1)}
                >
                  <ArrowRight className="h-4 w-4" />
                  {t("moveRight")}
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          )
        })}
      </Reorder.Group>
      <button
        type="button"
        disabled={loading || busy}
        onClick={() => void create()}
        className="m-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none transition-colors hover:bg-background/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
        title={t("create")}
        aria-label={t("create")}
      >
        {creating || (loading && !hydrated) ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Plus className="h-4 w-4" />
        )}
      </button>
      <div data-tauri-drag-region className="h-full min-w-8 flex-1" />
      {rightInset > 0 && (
        <div
          data-tauri-drag-region
          className="h-full shrink-0"
          style={{ width: rightInset }}
        />
      )}
    </div>
  )
}

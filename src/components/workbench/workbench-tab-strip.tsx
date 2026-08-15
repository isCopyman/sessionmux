"use client"

import { useEffect, useRef, useState } from "react"
import { Reorder } from "motion/react"
import {
  ArrowLeft,
  ArrowRight,
  Clock3,
  Copy,
  Loader2,
  PanelsTopLeft,
  Pin,
  PinOff,
  Plus,
  X,
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import type { WorkbenchInfo } from "@/lib/types"

interface WorkbenchTabStripProps {
  leftInset: number
  rightInset: number
}

function mergeOpenWorkbenchOrder(
  allItems: WorkbenchInfo[],
  orderedOpenIds: readonly number[],
  openIds: ReadonlySet<number>
): WorkbenchInfo[] {
  const byId = new Map(allItems.map((item) => [item.id, item]))
  const orderedOpen = orderedOpenIds.flatMap((id) => {
    const item = byId.get(id)
    return item ? [item] : []
  })
  let openIndex = 0
  return allItems.map((item) =>
    openIds.has(item.id) ? (orderedOpen[openIndex++] ?? item) : item
  )
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
  const openIds = useWorkbenchStore((state) => state.openIds)
  const recentlyClosedIds = useWorkbenchStore(
    (state) => state.recentlyClosedIds
  )
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
  const ensureOpen = useWorkbenchStore((state) => state.ensureOpen)
  const closeView = useWorkbenchStore((state) => state.closeView)
  const reopenAndSwitch = useWorkbenchStore((state) => state.reopenAndSwitch)
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const switchingWorkbenchId = useTabStore(
    (state) => state.switchingWorkbenchId
  )
  const switchWorkbench = useTabStore((state) => state.switchWorkbench)
  const [creating, setCreating] = useState(false)
  const [copyingId, setCopyingId] = useState<number | null>(null)
  const [ordering, setOrdering] = useState(false)
  const [pinningId, setPinningId] = useState<number | null>(null)
  const [closingId, setClosingId] = useState<number | null>(null)
  const [reopeningId, setReopeningId] = useState<number | null>(null)
  const dragStartOrderRef = useRef<string | null>(null)
  const dragClickGuardRef = useRef<number | null>(null)
  const busy =
    switchingWorkbenchId != null ||
    creating ||
    copyingId != null ||
    pinningId != null ||
    closingId != null ||
    reopeningId != null ||
    ordering

  const openIdSet = new Set(openIds)
  // A switch triggered from the sidebar may finish one render before the local
  // mounted-tab preference is updated. Never let the active Workbench vanish.
  openIdSet.add(activeWorkbenchId)
  const openItems = items.filter((item) => openIdSet.has(item.id))
  const recentlyClosedItems = recentlyClosedIds.flatMap((id) => {
    const item = items.find((candidate) => candidate.id === id)
    return item ? [item] : []
  })

  useEffect(() => {
    if (hydrated) return
    void hydrate().catch((error) =>
      toast.error(t("loadFailed", { message: toErrorMessage(error) }))
    )
  }, [hydrate, hydrated, t])

  useEffect(() => {
    if (!hydrated) return
    ensureOpen(activeWorkbenchId)
  }, [activeWorkbenchId, ensureOpen, hydrated])

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
    const index = openItems.findIndex((candidate) => candidate.id === item.id)
    const target = index + delta
    if (index < 0 || target < 0 || target >= openItems.length) return
    if (openItems[target].is_pinned !== item.is_pinned) return
    const next = [...openItems]
    const current = next[index]
    next[index] = next[target]
    next[target] = current
    previewOrder(
      mergeOpenWorkbenchOrder(
        items,
        next.map((candidate) => candidate.id),
        openIdSet
      )
    )
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

  const close = async (item: WorkbenchInfo) => {
    if (busy || openItems.length <= 1) return
    setClosingId(item.id)
    try {
      await closeView(item.id)
    } catch (error) {
      toast.error(t("closeFailed", { message: toErrorMessage(error) }))
    } finally {
      setClosingId(null)
    }
  }

  const reopen = async (item: WorkbenchInfo) => {
    if (busy) return
    setReopeningId(item.id)
    try {
      await reopenAndSwitch(item.id)
    } catch (error) {
      toast.error(t("switchFailed", { message: toErrorMessage(error) }))
    } finally {
      setReopeningId(null)
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
        values={openItems.map((item) => item.id)}
        onReorder={(orderedIds) => {
          previewOrder(mergeOpenWorkbenchOrder(items, orderedIds, openIdSet))
        }}
        className="flex min-w-0 items-end gap-0.5 overflow-x-auto px-1 pt-1"
      >
        {openItems.map((item, index) => {
          const active = item.id === activeWorkbenchId
          const restoring = item.id === switchingWorkbenchId
          const canMoveLeft =
            index > 0 && openItems[index - 1].is_pinned === item.is_pinned
          const canMoveRight =
            index < openItems.length - 1 &&
            openItems[index + 1].is_pinned === item.is_pinned
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
                    dragStartOrderRef.current = openItems
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
                      .items.filter((candidate) =>
                        useWorkbenchStore
                          .getState()
                          .openIds.includes(candidate.id)
                      )
                      .map((candidate) => candidate.id)
                      .join(",")
                    if (currentOrder === dragStartOrderRef.current) return
                    void commitOrder()
                  }}
                  className={cn(
                    "group flex h-9 max-w-52 min-w-24 shrink-0 items-stretch rounded-t-md border border-transparent outline-none transition-colors",
                    "hover:bg-background/55 focus-within:ring-2 focus-within:ring-ring focus-within:ring-inset",
                    active &&
                      "border-border/70 border-b-background bg-background text-foreground",
                    !active && "text-muted-foreground",
                    busy && "opacity-70"
                  )}
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
                    className="flex min-w-0 flex-1 items-center gap-2 rounded-tl-md py-0 pl-3 pr-1 text-xs font-medium outline-none"
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
                  <button
                    type="button"
                    disabled={busy || openItems.length <= 1}
                    aria-label={t("closeTab", { name: item.name })}
                    title={t("close")}
                    onPointerDown={(event) => event.stopPropagation()}
                    onClick={(event) => {
                      event.stopPropagation()
                      void close(item)
                    }}
                    className={cn(
                      "mr-1 flex w-6 shrink-0 items-center justify-center self-center rounded-sm p-1 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring",
                      !active &&
                        "opacity-0 group-hover:opacity-100 focus:opacity-100",
                      openItems.length <= 1 && "invisible"
                    )}
                  >
                    {closingId === item.id ? (
                      <Loader2 className="h-3 w-3 animate-spin" />
                    ) : (
                      <X className="h-3 w-3" />
                    )}
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
                <ContextMenuSeparator />
                <ContextMenuItem
                  disabled={busy || openItems.length <= 1}
                  onSelect={() => void close(item)}
                >
                  <X className="h-4 w-4" />
                  {t("close")}
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
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            disabled={busy || recentlyClosedItems.length === 0}
            className="my-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none transition-colors hover:bg-background/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-35"
            title={t("recentlyClosed")}
            aria-label={t("recentlyClosed")}
          >
            {reopeningId != null ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Clock3 className="h-4 w-4" />
            )}
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          {recentlyClosedItems.map((item) => (
            <DropdownMenuItem
              key={item.id}
              disabled={busy}
              onSelect={() => void reopen(item)}
            >
              <PanelsTopLeft className="h-4 w-4" />
              <span className="max-w-64 truncate">{item.name}</span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
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

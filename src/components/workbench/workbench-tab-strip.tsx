"use client"

import { useEffect, useState } from "react"
import { Loader2, PanelsTopLeft, Plus } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { useTabStore } from "@/contexts/tab-context"
import { useWorkbenchStore } from "@/stores/workbench-store"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"

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
  const items = useWorkbenchStore((state) => state.items)
  const hydrated = useWorkbenchStore((state) => state.hydrated)
  const loading = useWorkbenchStore((state) => state.loading)
  const hydrate = useWorkbenchStore((state) => state.hydrate)
  const createAndSwitch = useWorkbenchStore((state) => state.createAndSwitch)
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const switchingWorkbenchId = useTabStore(
    (state) => state.switchingWorkbenchId
  )
  const switchWorkbench = useTabStore((state) => state.switchWorkbench)
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    if (hydrated) return
    void hydrate().catch((error) =>
      toast.error(t("loadFailed", { message: toErrorMessage(error) }))
    )
  }, [hydrate, hydrated, t])

  const create = async () => {
    if (creating || switchingWorkbenchId != null) return
    setCreating(true)
    try {
      await createAndSwitch(t("defaultName", { number: items.length + 1 }))
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setCreating(false)
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
      <div
        role="tablist"
        aria-label={t("switchTitle")}
        className="flex min-w-0 items-end gap-0.5 overflow-x-auto px-1 pt-1"
      >
        {items.map((item) => {
          const active = item.id === activeWorkbenchId
          const restoring = item.id === switchingWorkbenchId
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={active}
              data-workbench-tab-id={item.id}
              disabled={switchingWorkbenchId != null}
              onClick={() => {
                if (active) return
                void switchWorkbench(item.id).catch((error) =>
                  toast.error(
                    t("switchFailed", { message: toErrorMessage(error) })
                  )
                )
              }}
              className={cn(
                "group flex h-9 max-w-52 min-w-24 items-center gap-2 rounded-t-md border border-transparent px-3 text-xs font-medium outline-none transition-colors",
                "hover:bg-background/55 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                active &&
                  "border-border/70 border-b-background bg-background text-foreground",
                !active && "text-muted-foreground",
                switchingWorkbenchId != null && "opacity-70"
              )}
              title={item.name}
            >
              {restoring ? (
                <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
              ) : (
                <PanelsTopLeft
                  className={cn(
                    "h-3.5 w-3.5 shrink-0",
                    active && "text-primary"
                  )}
                />
              )}
              <span className="truncate">{item.name}</span>
            </button>
          )
        })}
      </div>
      <button
        type="button"
        disabled={loading || creating || switchingWorkbenchId != null}
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

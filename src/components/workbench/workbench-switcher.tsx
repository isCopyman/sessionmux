"use client"

import { useEffect, useState } from "react"
import {
  Check,
  ChevronDown,
  Loader2,
  PanelsTopLeft,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { useTabStore } from "@/contexts/tab-context"
import { useWorkbenchStore } from "@/stores/workbench-store"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"

type EditMode = "create" | "rename" | null

export function WorkbenchSwitcher() {
  const t = useTranslations("Folder.workbench")
  const items = useWorkbenchStore((state) => state.items)
  const hydrated = useWorkbenchStore((state) => state.hydrated)
  const loading = useWorkbenchStore((state) => state.loading)
  const hydrate = useWorkbenchStore((state) => state.hydrate)
  const createAndSwitch = useWorkbenchStore((state) => state.createAndSwitch)
  const rename = useWorkbenchStore((state) => state.rename)
  const remove = useWorkbenchStore((state) => state.remove)
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const switching = useTabStore((state) => state.switchingWorkbench)
  const switchWorkbench = useTabStore((state) => state.switchWorkbench)
  const active = items.find((item) => item.id === activeWorkbenchId)

  const [editMode, setEditMode] = useState<EditMode>(null)
  const [name, setName] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)

  useEffect(() => {
    if (hydrated) return
    void hydrate().catch((error) =>
      toast.error(t("loadFailed", { message: toErrorMessage(error) }))
    )
  }, [hydrate, hydrated, t])

  const openCreate = () => {
    setName(t("defaultName", { number: items.length + 1 }))
    setEditMode("create")
  }
  const openRename = () => {
    if (!active) return
    setName(active.name)
    setEditMode("rename")
  }
  const submit = async () => {
    const value = name.trim()
    if (!value || submitting) return
    setSubmitting(true)
    try {
      if (editMode === "create") await createAndSwitch(value)
      else if (editMode === "rename" && active) await rename(active.id, value)
      setEditMode(null)
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setSubmitting(false)
    }
  }
  const confirmDelete = async () => {
    if (!active) return
    setSubmitting(true)
    try {
      await remove(active.id)
      setDeleteOpen(false)
    } catch (error) {
      toast.error(t("deleteFailed", { message: toErrorMessage(error) }))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            disabled={loading || switching}
            className={cn(
              "group flex h-8 w-full items-center gap-2 rounded-full px-2 text-left",
              "text-[0.8125rem] font-medium outline-none transition-colors",
              "hover:bg-sidebar-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
              (loading || switching) && "opacity-60"
            )}
            title={t("switchTitle")}
          >
            {switching ? (
              <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
            ) : (
              <PanelsTopLeft className="h-3.5 w-3.5 shrink-0 text-primary" />
            )}
            <span className="min-w-0 flex-1 truncate">
              {active?.name ?? t("loading")}
            </span>
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="min-w-60">
          {items.map((item) => (
            <DropdownMenuItem
              key={item.id}
              disabled={switching}
              onSelect={() => {
                if (item.id === activeWorkbenchId) return
                void switchWorkbench(item.id).catch((error) =>
                  toast.error(
                    t("switchFailed", { message: toErrorMessage(error) })
                  )
                )
              }}
            >
              <span className="flex h-4 w-4 items-center justify-center">
                {item.id === activeWorkbenchId && <Check className="h-4 w-4" />}
              </span>
              <span className="truncate">{item.name}</span>
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={openCreate}>
            <Plus className="h-4 w-4" />
            {t("create")}
          </DropdownMenuItem>
          <DropdownMenuItem disabled={!active} onSelect={openRename}>
            <Pencil className="h-4 w-4" />
            {t("rename")}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={!active || items.length <= 1}
            variant="destructive"
            onSelect={() => setDeleteOpen(true)}
          >
            <Trash2 className="h-4 w-4" />
            {t("delete")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Dialog
        open={editMode !== null}
        onOpenChange={(open) => !open && setEditMode(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {editMode === "create" ? t("createTitle") : t("renameTitle")}
            </DialogTitle>
          </DialogHeader>
          <Input
            autoFocus
            value={name}
            maxLength={80}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void submit()
            }}
            placeholder={t("namePlaceholder")}
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditMode(null)}>
              {t("cancel")}
            </Button>
            <Button
              disabled={!name.trim() || submitting}
              onClick={() => void submit()}
            >
              {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
              {editMode === "create" ? t("createAndSwitch") : t("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("deleteDescription", { name: active?.name ?? "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={submitting}
              onClick={(event) => {
                event.preventDefault()
                void confirmDelete()
              }}
            >
              {t("delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

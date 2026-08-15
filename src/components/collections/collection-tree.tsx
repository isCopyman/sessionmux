"use client"

import { useEffect, useMemo, useState, type ReactNode } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  Inbox,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react"
import { Button } from "@/components/ui/button"
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type { CollectionInfo } from "@/lib/types"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"
import { useCollectionStore } from "@/stores/collection-store"

type OpenScope = number | "unclassified"
type EditorState =
  | { mode: "create"; parentId: number | null }
  | { mode: "rename"; item: CollectionInfo }
  | { mode: "move"; item: CollectionInfo }

interface CollectionTreeProps {
  onOpenScope: (scope: OpenScope) => void
}

function descendants(items: CollectionInfo[], id: number) {
  const result = new Set<number>([id])
  let changed = true
  while (changed) {
    changed = false
    for (const item of items) {
      if (
        item.parent_id != null &&
        result.has(item.parent_id) &&
        !result.has(item.id)
      ) {
        result.add(item.id)
        changed = true
      }
    }
  }
  return result
}

function orderedChildren(items: CollectionInfo[]) {
  const result = new Map<number | null, CollectionInfo[]>()
  for (const item of items) {
    const siblings = result.get(item.parent_id) ?? []
    siblings.push(item)
    result.set(item.parent_id, siblings)
  }
  for (const siblings of result.values()) {
    siblings.sort((a, b) => a.position - b.position || a.id - b.id)
  }
  return result
}

function flatOptions(items: CollectionInfo[]) {
  const children = orderedChildren(items)
  const result: Array<{ item: CollectionInfo; depth: number }> = []
  const seen = new Set<number>()
  const append = (item: CollectionInfo, depth: number) => {
    if (seen.has(item.id)) return
    seen.add(item.id)
    result.push({ item, depth })
    for (const child of children.get(item.id) ?? []) append(child, depth + 1)
  }
  for (const root of children.get(null) ?? []) append(root, 0)
  for (const item of items) if (!seen.has(item.id)) append(item, 0)
  return result
}

export function CollectionTree({ onOpenScope }: CollectionTreeProps) {
  const t = useTranslations("Folder.sidebar.collections")
  const tCommon = useTranslations("Folder.common")
  const items = useCollectionStore((state) => state.items)
  const hydrated = useCollectionStore((state) => state.hydrated)
  const loading = useCollectionStore((state) => state.loading)
  const hydrate = useCollectionStore((state) => state.hydrate)
  const create = useCollectionStore((state) => state.create)
  const rename = useCollectionStore((state) => state.rename)
  const move = useCollectionStore((state) => state.move)
  const remove = useCollectionStore((state) => state.remove)

  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [name, setName] = useState("")
  const [parentId, setParentId] = useState<number | null>(null)
  const [pending, setPending] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<CollectionInfo | null>(null)

  useEffect(() => {
    if (!hydrated) {
      void hydrate().catch((error) => {
        toast.error(t("operationFailed", { message: toErrorMessage(error) }))
      })
    }
  }, [hydrate, hydrated, t])

  const children = useMemo(() => orderedChildren(items), [items])
  const options = useMemo(() => flatOptions(items), [items])
  const invalidMoveParents = useMemo(
    () =>
      editor?.mode === "move"
        ? descendants(items, editor.item.id)
        : new Set<number>(),
    [editor, items]
  )

  const openEditor = (next: EditorState) => {
    setEditor(next)
    if (next.mode === "create") {
      setName("")
      setParentId(next.parentId)
    } else {
      setName(next.item.name)
      setParentId(next.item.parent_id)
    }
  }

  const submitEditor = async () => {
    if (!editor) return
    const normalizedName = name.trim()
    if (editor.mode !== "move" && !normalizedName) return
    setPending(true)
    try {
      if (editor.mode === "create") {
        const created = await create(normalizedName, parentId)
        if (parentId != null) {
          setExpanded((current) => new Set(current).add(parentId))
        }
        onOpenScope(created.id)
      } else if (editor.mode === "rename") {
        await rename(editor.item.id, normalizedName)
      } else {
        await move(editor.item.id, parentId)
        if (parentId != null) {
          setExpanded((current) => new Set(current).add(parentId))
        }
      }
      setEditor(null)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  const confirmDelete = async () => {
    if (!deleteTarget) return
    setPending(true)
    try {
      await remove(deleteTarget.id)
      setDeleteTarget(null)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  const renderItems = (parent: number | null, depth = 0): ReactNode =>
    (children.get(parent) ?? []).map((item) => {
      const childItems = children.get(item.id) ?? []
      const isExpanded = expanded.has(item.id)
      return (
        <div key={item.id}>
          <div
            className="group flex h-7 min-w-0 items-center rounded-md pe-1 hover:bg-sidebar-accent"
            style={{ paddingInlineStart: `${0.25 + depth * 0.75}rem` }}
          >
            <button
              type="button"
              className={cn(
                "flex h-6 w-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground",
                childItems.length === 0 && "pointer-events-none opacity-0"
              )}
              aria-label={isExpanded ? t("collapse") : t("expand")}
              onClick={() =>
                setExpanded((current) => {
                  const next = new Set(current)
                  if (next.has(item.id)) next.delete(item.id)
                  else next.add(item.id)
                  return next
                })
              }
            >
              {isExpanded ? (
                <ChevronDown className="h-3.5 w-3.5" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 rtl:rotate-180" />
              )}
            </button>
            <button
              type="button"
              className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-start text-xs"
              title={item.name}
              onClick={() => onOpenScope(item.id)}
            >
              {isExpanded ? (
                <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              ) : (
                <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              )}
              <span className="truncate">{item.name}</span>
            </button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                  aria-label={t("actions", { name: item.name })}
                >
                  <MoreHorizontal className="h-3.5 w-3.5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem
                  onSelect={() =>
                    openEditor({ mode: "create", parentId: item.id })
                  }
                >
                  <FolderPlus className="h-4 w-4" />
                  {t("newChild")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => openEditor({ mode: "rename", item })}
                >
                  <Pencil className="h-4 w-4" />
                  {t("rename")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => openEditor({ mode: "move", item })}
                >
                  <FolderInput className="h-4 w-4" />
                  {t("move")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() => setDeleteTarget(item)}
                >
                  <Trash2 className="h-4 w-4" />
                  {t("delete")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
          {isExpanded ? renderItems(item.id, depth + 1) : null}
        </div>
      )
    })

  return (
    <section className="shrink-0 border-b border-border/40 px-1.5 pb-1.5">
      <div className="flex h-7 items-center px-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">{t("title")}</span>
        <Button
          size="icon-sm"
          variant="ghost"
          className="h-6 w-6"
          aria-label={t("newRoot")}
          onClick={() => openEditor({ mode: "create", parentId: null })}
        >
          <Plus className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div className="max-h-40 overflow-y-auto">
        <button
          type="button"
          className="flex h-7 w-full items-center gap-1.5 rounded-md px-2 text-start text-xs hover:bg-sidebar-accent"
          onClick={() => onOpenScope("unclassified")}
        >
          <Inbox className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate">{t("unclassified")}</span>
        </button>
        {loading && items.length === 0 ? (
          <div className="flex h-8 items-center justify-center text-muted-foreground">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          </div>
        ) : items.length === 0 ? (
          <button
            type="button"
            className="w-full px-2 py-1.5 text-start text-xs text-muted-foreground hover:text-foreground"
            onClick={() => openEditor({ mode: "create", parentId: null })}
          >
            {t("empty")}
          </button>
        ) : (
          renderItems(null)
        )}
      </div>

      <Dialog
        open={editor != null}
        onOpenChange={(open) => !open && setEditor(null)}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>
              {editor?.mode === "create"
                ? t("createTitle")
                : editor?.mode === "rename"
                  ? t("renameTitle")
                  : t("moveTitle")}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            {editor?.mode !== "move" ? (
              <Input
                autoFocus
                value={name}
                maxLength={80}
                placeholder={t("namePlaceholder")}
                onChange={(event) => setName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void submitEditor()
                }}
              />
            ) : null}
            {editor?.mode !== "rename" ? (
              <Select
                value={parentId == null ? "root" : String(parentId)}
                onValueChange={(value) =>
                  setParentId(value === "root" ? null : Number(value))
                }
              >
                <SelectTrigger aria-label={t("parentLabel")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="root">{t("root")}</SelectItem>
                  {options
                    .filter(({ item }) => !invalidMoveParents.has(item.id))
                    .map(({ item, depth }) => (
                      <SelectItem key={item.id} value={String(item.id)}>
                        {depth > 0 ? `${"· ".repeat(depth)}` : ""}
                        {item.name}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            ) : null}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditor(null)}>
              {tCommon("cancel")}
            </Button>
            <Button
              disabled={pending || (editor?.mode !== "move" && !name.trim())}
              onClick={() => void submitEditor()}
            >
              {pending ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              {tCommon("confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={deleteTarget != null}
        onOpenChange={(open) => !open && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("deleteDescription", { name: deleteTarget?.name ?? "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={pending}
              onClick={() => void confirmDelete()}
            >
              {tCommon("confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  )
}

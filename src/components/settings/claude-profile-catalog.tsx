"use client"

import { useCallback, useEffect, useState } from "react"
import { Loader2, Pencil, Plus, Star, Trash2 } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { Badge } from "@/components/ui/badge"
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
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  claudeProfileDelete,
  claudeProfileList,
  claudeProfileUpsert,
} from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import {
  FOLLOW_DEFAULT_CLAUDE_PROFILE_ID,
  isValidClaudeProfileId,
  type ClaudeProfileInfo,
  type ClaudeProfileKind,
  type ClaudeProfileUpsert,
} from "@/lib/types"

type EditableKind = ClaudeProfileUpsert["kind"]

interface ClaudeProfileCatalogProps {
  defaultProfileId: string
  onSetAgentDefault: (profileId: string) => Promise<void>
}

interface FormState {
  id: string
  label: string
  kind: EditableKind
  configDir: string
  baseUrl: string
  authToken: string
  model: string
}

const EMPTY_FORM: FormState = {
  id: "",
  label: "",
  kind: "managed",
  configDir: "",
  baseUrl: "",
  authToken: "",
  model: "",
}

function isAbsolutePath(value: string): boolean {
  const trimmed = value.trim()
  if (!trimmed) return false
  if (trimmed.startsWith("/") || trimmed.startsWith("\\\\")) return true
  return /^[a-zA-Z]:[\\/]/.test(trimmed)
}

function kindLabel(
  kind: ClaudeProfileKind,
  t: (key: "kindConfigDir" | "kindManaged" | "kindFollowDefault") => string
): string {
  if (kind === "configDir") return t("kindConfigDir")
  if (kind === "managed") return t("kindManaged")
  return t("kindFollowDefault")
}

function rowDetail(profile: ClaudeProfileInfo): string | null {
  if (profile.kind === "configDir") {
    return profile.configDir?.trim() || null
  }
  if (profile.kind === "managed") {
    const parts = [profile.baseUrl?.trim(), profile.authTokenMasked?.trim()]
    return parts.filter(Boolean).join(" · ") || null
  }
  return null
}

export function ClaudeProfileCatalog({
  defaultProfileId,
  onSetAgentDefault,
}: ClaudeProfileCatalogProps) {
  const t = useTranslations("AcpAgentSettings.claudeProfile")
  const tActions = useTranslations("AcpAgentSettings.actions")
  const [profiles, setProfiles] = useState<ClaudeProfileInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [editor, setEditor] = useState<"create" | ClaudeProfileInfo | null>(
    null
  )
  const [form, setForm] = useState<FormState>(EMPTY_FORM)
  const [formError, setFormError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<ClaudeProfileInfo | null>(
    null
  )
  const [deleting, setDeleting] = useState(false)
  const [settingDefaultId, setSettingDefaultId] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    const list = await claudeProfileList()
    setProfiles(list)
  }, [])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    void refresh()
      .catch((error: unknown) => {
        if (cancelled) return
        toast.error(t("listFailed"), {
          description: toErrorMessage(error),
        })
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [refresh, t])

  const openCreate = useCallback(() => {
    setForm(EMPTY_FORM)
    setFormError(null)
    setEditor("create")
  }, [])

  const openEdit = useCallback((profile: ClaudeProfileInfo) => {
    setForm({
      id: profile.id,
      label: profile.label,
      kind: profile.kind === "configDir" ? "configDir" : "managed",
      configDir: profile.configDir ?? "",
      baseUrl: profile.baseUrl ?? "",
      authToken: "",
      model: profile.model ?? "",
    })
    setFormError(null)
    setEditor(profile)
  }, [])

  const closeEditor = useCallback(() => {
    if (saving) return
    setEditor(null)
    setFormError(null)
  }, [saving])

  const handleSave = useCallback(async () => {
    const id = form.id.trim()
    const label = form.label.trim()
    if (!isValidClaudeProfileId(id)) {
      setFormError(
        id === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
          ? t("idReserved")
          : t("idInvalid")
      )
      return
    }
    if (!label) {
      setFormError(t("labelRequired"))
      return
    }
    if (form.kind === "configDir" && !isAbsolutePath(form.configDir)) {
      setFormError(t("configDirRequired"))
      return
    }

    const payload: ClaudeProfileUpsert = {
      id,
      label,
      kind: form.kind,
    }
    if (form.kind === "configDir") {
      payload.configDir = form.configDir.trim()
    } else {
      payload.baseUrl = form.baseUrl.trim() || null
      payload.model = form.model.trim() || null
      const token = form.authToken.trim()
      if (token) payload.authToken = token
    }

    setSaving(true)
    setFormError(null)
    try {
      const saved = await claudeProfileUpsert(payload)
      setProfiles((prev) => {
        const index = prev.findIndex((item) => item.id === saved.id)
        if (index >= 0) {
          const next = [...prev]
          next[index] = saved
          return next
        }
        const head = prev[0]?.id === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID ? 1 : 0
        return [...prev.slice(0, head), saved, ...prev.slice(head)]
      })
      toast.success(t("saveSuccess"))
      setEditor(null)
    } catch (error: unknown) {
      setFormError(toErrorMessage(error) || t("saveFailed"))
    } finally {
      setSaving(false)
    }
  }, [form, t])

  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return
    setDeleting(true)
    try {
      const id = deleteTarget.id
      await claudeProfileDelete(id)
      setProfiles((prev) => prev.filter((item) => item.id !== id))
      toast.success(t("deleteSuccess"))
      setDeleteTarget(null)
    } catch (error: unknown) {
      toast.error(t("deleteFailed"), {
        description: toErrorMessage(error),
      })
    } finally {
      setDeleting(false)
    }
  }, [deleteTarget, t])

  const handleSetDefault = useCallback(
    async (profileId: string) => {
      setSettingDefaultId(profileId)
      try {
        await onSetAgentDefault(profileId)
        toast.success(t("setAsDefaultSuccess"))
      } catch (error: unknown) {
        toast.error(t("setAsDefaultFailed"), {
          description: toErrorMessage(error),
        })
      } finally {
        setSettingDefaultId(null)
      }
    },
    [onSetAgentDefault, t]
  )

  const editing = editor !== null && editor !== "create"
  const currentDefault =
    defaultProfileId.trim() || FOLLOW_DEFAULT_CLAUDE_PROFILE_ID

  return (
    <div className="space-y-2">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <label className="text-xs font-medium">{t("sectionTitle")}</label>
          <p className="mt-1 text-[11px] text-muted-foreground">
            {t("sectionDescription")}
          </p>
        </div>
        <Button type="button" variant="outline" size="sm" onClick={openCreate}>
          <Plus className="h-3.5 w-3.5" />
          {t("new")}
        </Button>
      </div>

      {loading ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("loading")}
        </div>
      ) : (
        <ul className="divide-y rounded-md border bg-background/50">
          {profiles.map((profile) => {
            const isFollowDefault =
              profile.id === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
            const isDefault = profile.id === currentDefault
            return (
              <li
                key={profile.id}
                className="flex items-start justify-between gap-2 px-3 py-2"
              >
                <div className="min-w-0 space-y-1">
                  <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <span className="truncate text-sm font-medium">
                      {isFollowDefault ? t("followDefault") : profile.label}
                    </span>
                    <Badge variant="outline">
                      {kindLabel(profile.kind, t)}
                    </Badge>
                    {isDefault ? (
                      <Badge variant="secondary">{t("currentDefault")}</Badge>
                    ) : null}
                  </div>
                  {rowDetail(profile) ? (
                    <p className="truncate text-[11px] text-muted-foreground">
                      {rowDetail(profile)}
                    </p>
                  ) : null}
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {!isDefault ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      disabled={settingDefaultId === profile.id}
                      onClick={() => {
                        void handleSetDefault(profile.id)
                      }}
                      title={t("setAsDefault")}
                    >
                      {settingDefaultId === profile.id ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Star className="h-3.5 w-3.5" />
                      )}
                      {t("setAsDefault")}
                    </Button>
                  ) : null}
                  {!isFollowDefault ? (
                    <>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        onClick={() => openEdit(profile)}
                        title={t("edit")}
                        aria-label={t("edit")}
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        onClick={() => setDeleteTarget(profile)}
                        title={t("delete")}
                        aria-label={t("delete")}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </>
                  ) : null}
                </div>
              </li>
            )
          })}
        </ul>
      )}

      <Dialog
        open={editor !== null}
        onOpenChange={(open) => {
          if (!open) closeEditor()
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {editing ? t("editTitle") : t("newTitle")}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <label
                htmlFor="claude-profile-id"
                className="text-[11px] text-muted-foreground"
              >
                {t("fieldId")}
              </label>
              <Input
                id="claude-profile-id"
                value={form.id}
                readOnly={editing}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, id: event.target.value }))
                }
                placeholder="api"
              />
            </div>
            <div className="space-y-1.5">
              <label
                htmlFor="claude-profile-label"
                className="text-[11px] text-muted-foreground"
              >
                {t("fieldLabel")}
              </label>
              <Input
                id="claude-profile-label"
                value={form.label}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, label: event.target.value }))
                }
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-[11px] text-muted-foreground">
                {t("fieldKind")}
              </label>
              <Select
                value={form.kind}
                onValueChange={(value) => {
                  if (value === "configDir" || value === "managed") {
                    setForm((prev) => ({ ...prev, kind: value }))
                  }
                }}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="start">
                  <SelectItem value="configDir">
                    {t("kindConfigDir")}
                  </SelectItem>
                  <SelectItem value="managed">{t("kindManaged")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {form.kind === "configDir" ? (
              <div className="space-y-1.5">
                <label className="text-[11px] text-muted-foreground">
                  {t("fieldConfigDir")}
                </label>
                <Input
                  value={form.configDir}
                  onChange={(event) =>
                    setForm((prev) => ({
                      ...prev,
                      configDir: event.target.value,
                    }))
                  }
                  placeholder="/home/user/.claude-work"
                />
              </div>
            ) : (
              <>
                <div className="space-y-1.5">
                  <label className="text-[11px] text-muted-foreground">
                    {t("fieldBaseUrl")}
                  </label>
                  <Input
                    value={form.baseUrl}
                    onChange={(event) =>
                      setForm((prev) => ({
                        ...prev,
                        baseUrl: event.target.value,
                      }))
                    }
                    placeholder="https://api.example.com"
                  />
                </div>
                <div className="space-y-1.5">
                  <label className="text-[11px] text-muted-foreground">
                    {t("fieldAuthToken")}
                  </label>
                  <Input
                    type="password"
                    value={form.authToken}
                    onChange={(event) =>
                      setForm((prev) => ({
                        ...prev,
                        authToken: event.target.value,
                      }))
                    }
                    placeholder={t("tokenKeepPlaceholder")}
                    autoComplete="off"
                  />
                </div>
                <div className="space-y-1.5">
                  <label className="text-[11px] text-muted-foreground">
                    {t("fieldModel")}
                  </label>
                  <Input
                    value={form.model}
                    onChange={(event) =>
                      setForm((prev) => ({
                        ...prev,
                        model: event.target.value,
                      }))
                    }
                  />
                </div>
              </>
            )}
            {formError ? (
              <p className="text-xs text-destructive">{formError}</p>
            ) : null}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={closeEditor}
              disabled={saving}
            >
              {tActions("cancel")}
            </Button>
            <Button
              type="button"
              onClick={() => void handleSave()}
              disabled={saving}
            >
              {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
              {t("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open && !deleting) setDeleteTarget(null)
        }}
      >
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("deleteDescription", { label: deleteTarget?.label ?? "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>
              {tActions("cancel")}
            </AlertDialogCancel>
            <Button
              variant="destructive"
              onClick={() => void handleDelete()}
              disabled={deleting}
            >
              {deleting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Trash2 className="h-3.5 w-3.5" />
              )}
              {tActions("confirmDelete")}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  ChevronRight,
  Copy,
  FileDown,
  Loader2,
  Plus,
  Star,
  Trash2,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  AlertDialog,
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
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import {
  claudeProfileDelete,
  claudeProfileList,
  claudeProfileUpsert,
  claudeSettingsRead,
} from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import {
  FOLLOW_DEFAULT_CLAUDE_PROFILE_ID,
  isValidClaudeProfileId,
  type ClaudeProfileInfo,
  type ClaudeProfileUpsert,
} from "@/lib/types"

type EditableKind = ClaudeProfileUpsert["kind"]

interface ClaudeProfileCatalogProps {
  defaultProfileId: string
  onSetAgentDefault: (profileId: string) => Promise<void>
  /**
   * Which tab is open. The parent renders the CLI-global connection settings
   * as the body of the "Follow default" tab, the way VS Code shows the User
   * settings under the User tab, so it has to know which tab won.
   */
  onActiveProfileChange?: (profileId: string) => void
}

interface FormState {
  id: string
  label: string
  kind: EditableKind
  configDir: string
  baseUrl: string
  authToken: string
  model: string
  /** The profile's settings.json, as text. `""` means "no base". */
  settingsJson: string
}

/** A tab's edit buffer. `isNew` profiles exist only in the browser until saved. */
interface Draft extends FormState {
  isNew: boolean
}

/**
 * File-less profiles: `follow-default` and `official-direct`. They have no
 * fields to edit and cannot be saved or deleted, so every "can I edit this
 * tab" question routes through here rather than comparing ids one by one.
 * `isVirtual` is the backend's own flag; the kinds are the fallback for a
 * server that predates it.
 */
function isVirtualProfile(profile: ClaudeProfileInfo): boolean {
  return (
    profile.isVirtual === true ||
    profile.kind === "followDefault" ||
    profile.kind === "officialDirect"
  )
}

function isAbsolutePath(value: string): boolean {
  const trimmed = value.trim()
  if (!trimmed) return false
  if (trimmed.startsWith("/") || trimmed.startsWith("\\\\")) return true
  return /^[a-zA-Z]:[\\/]/.test(trimmed)
}

/** Edit buffer seeded from a saved profile. The token is never echoed back
 *  (the API only returns a mask), so blank means "keep what is stored". */
function draftFromProfile(profile: ClaudeProfileInfo): Draft {
  return {
    id: profile.id,
    label: profile.label,
    kind: profile.kind === "configDir" ? "configDir" : "managed",
    configDir: profile.configDir ?? "",
    baseUrl: profile.baseUrl ?? "",
    authToken: "",
    model: profile.model ?? "",
    settingsJson: profile.settingsJson ?? "",
    isNew: false,
  }
}

function isDirty(
  draft: Draft,
  profile: ClaudeProfileInfo | undefined
): boolean {
  if (draft.isNew || !profile) return true
  const base = draftFromProfile(profile)
  return (
    draft.label !== base.label ||
    draft.kind !== base.kind ||
    draft.configDir !== base.configDir ||
    draft.baseUrl !== base.baseUrl ||
    draft.model !== base.model ||
    draft.settingsJson !== base.settingsJson ||
    draft.authToken.trim() !== ""
  )
}

/** `Settings 2`, `Settings 3`, … — the first free number, so adding a profile
 *  never asks the user to invent a name (or an id) before they can type. */
function nextFreeSuffix(taken: Set<string>): number {
  for (let n = 2; n < 1000; n += 1) {
    if (!taken.has(`settings-${n}`)) return n
  }
  return Date.now() % 1000
}

export function ClaudeProfileCatalog({
  defaultProfileId,
  onSetAgentDefault,
  onActiveProfileChange,
}: ClaudeProfileCatalogProps) {
  const t = useTranslations("AcpAgentSettings.claudeProfile")
  const tActions = useTranslations("AcpAgentSettings.actions")
  const [profiles, setProfiles] = useState<ClaudeProfileInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [selectedId, setSelectedId] = useState<string>(
    FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
  )
  const [drafts, setDrafts] = useState<Record<string, Draft>>({})
  const [formError, setFormError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<ClaudeProfileInfo | null>(
    null
  )
  const [deleting, setDeleting] = useState(false)
  const [settingDefaultId, setSettingDefaultId] = useState<string | null>(null)
  const [copiedFrom, setCopiedFrom] = useState<string | null>(null)
  /** Secret keys the import could not carry, so the editor can name them. */
  const [droppedSecrets, setDroppedSecrets] = useState<string[]>([])
  const [importing, setImporting] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)

  // `t` is a fresh function identity on every render, so it must NOT be a
  // dependency of the load effect: that re-fetches on every render and the
  // late response overwrites a profile the user just saved.
  const tRef = useRef(t)
  tRef.current = t

  // Same reason as `tRef`: the parent passes an inline arrow, so depending on
  // the callback itself would fire this on every render.
  const notifyActiveRef = useRef(onActiveProfileChange)
  notifyActiveRef.current = onActiveProfileChange
  useEffect(() => {
    notifyActiveRef.current?.(selectedId)
  }, [selectedId])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    claudeProfileList()
      .then((list) => {
        if (cancelled) return
        setProfiles(list)
        setDrafts((prev) => {
          const next = { ...prev }
          for (const profile of list) {
            if (isVirtualProfile(profile)) continue
            if (!next[profile.id]) next[profile.id] = draftFromProfile(profile)
          }
          return next
        })
      })
      .catch((error: unknown) => {
        if (cancelled) return
        toast.error(tRef.current("listFailed"), {
          description: toErrorMessage(error),
        })
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const profileById = useMemo(
    () => new Map(profiles.map((profile) => [profile.id, profile])),
    [profiles]
  )

  /** Saved profiles first (server order pins Follow default at the head), then
   *  tabs the user just added and has not saved yet. */
  const tabs = useMemo(() => {
    const savedIds = new Set(profiles.map((profile) => profile.id))
    const pending = Object.values(drafts).filter(
      (draft) => draft.isNew && !savedIds.has(draft.id)
    )
    return [
      ...profiles.map((profile) => ({
        id: profile.id,
        // The draft's name wins so the tab renames as you type, the way a
        // renamed file tab does.
        label: isVirtualProfile(profile)
          ? profile.kind === "followDefault"
            ? t("followDefault")
            : t("officialDirect")
          : (drafts[profile.id]?.label ?? profile.label),
        isNew: false,
      })),
      ...pending.map((draft) => ({
        id: draft.id,
        label: draft.label,
        isNew: true,
      })),
    ]
  }, [drafts, profiles, t])

  const selectedDraft = drafts[selectedId]
  const selectedProfile = profileById.get(selectedId)
  const isFollowDefaultTab = selectedId === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
  const isVirtualTab = selectedProfile
    ? isVirtualProfile(selectedProfile)
    : isFollowDefaultTab
  const currentDefault =
    defaultProfileId.trim() || FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
  const dirty = selectedDraft ? isDirty(selectedDraft, selectedProfile) : false

  const patchDraft = useCallback(
    (patch: Partial<FormState>) => {
      setFormError(null)
      setDrafts((prev) => {
        const current = prev[selectedId]
        if (!current) return prev
        return { ...prev, [selectedId]: { ...current, ...patch } }
      })
    },
    [selectedId]
  )

  /**
   * `source` duplicates an existing profile. The auth token is deliberately
   * NOT carried over: the API only ever hands the browser a mask, so copying
   * it would write the literal bullets back as a token.
   */
  const addProfile = useCallback(
    (source?: Draft, seedSettingsJson?: string) => {
      const taken = new Set([
        ...profiles.map((profile) => profile.id),
        ...Object.keys(drafts),
      ])
      const suffix = nextFreeSuffix(taken)
      const id = `settings-${suffix}`
      setDrafts((prev) => ({
        ...prev,
        [id]: {
          id,
          label: t("newProfileName", { n: suffix }),
          kind: source?.kind ?? "managed",
          configDir: source?.configDir ?? "",
          baseUrl: source?.baseUrl ?? "",
          authToken: "",
          model: source?.model ?? "",
          settingsJson: seedSettingsJson ?? source?.settingsJson ?? "",
          isNew: true,
        },
      }))
      setFormError(null)
      setCopiedFrom(source ? source.label : null)
      setDroppedSecrets([])
      setSelectedId(id)
    },
    [drafts, profiles, t]
  )

  /**
   * One-way import: read an existing settings.json and open it as a new
   * profile. codeg never writes back to the file it read, and the API masks
   * secret-looking `env` values, so those keys have to be retyped — which is
   * what `droppedSecretKeys` is for.
   */
  const importFromSettings = useCallback(async () => {
    setImporting(true)
    try {
      const result = await claudeSettingsRead(null)
      if (!result.exists) {
        toast.error(t("importMissing", { path: result.path }))
        return
      }
      addProfile(undefined, result.text)
      setDroppedSecrets(result.droppedSecretKeys)
      setSettingsOpen(true)
      toast.success(t("importSuccess", { path: result.path }))
    } catch (error: unknown) {
      toast.error(t("importFailed"), { description: toErrorMessage(error) })
    } finally {
      setImporting(false)
    }
  }, [addProfile, t])

  const discardNew = useCallback(() => {
    setDrafts((prev) => {
      const next = { ...prev }
      delete next[selectedId]
      return next
    })
    setFormError(null)
    setSelectedId(FOLLOW_DEFAULT_CLAUDE_PROFILE_ID)
  }, [selectedId])

  const handleSave = useCallback(async () => {
    const draft = drafts[selectedId]
    if (!draft) return
    const id = draft.id.trim()
    const label = draft.label.trim()
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
    if (draft.kind === "configDir" && !isAbsolutePath(draft.configDir)) {
      setFormError(t("configDirRequired"))
      return
    }

    const payload: ClaudeProfileUpsert = { id, label, kind: draft.kind }
    if (draft.kind === "configDir") {
      payload.configDir = draft.configDir.trim()
    } else {
      payload.baseUrl = draft.baseUrl.trim() || null
      payload.model = draft.model.trim() || null
      const token = draft.authToken.trim()
      if (token) payload.authToken = token
      // Parse before sending so a typo comes back as "line 7", not as an
      // opaque backend rejection. `""` is legal — it clears the base.
      const settings = draft.settingsJson.trim()
      if (settings) {
        try {
          const parsed: unknown = JSON.parse(settings)
          if (
            typeof parsed !== "object" ||
            parsed === null ||
            Array.isArray(parsed)
          ) {
            setFormError(t("settingsJsonNotObject"))
            return
          }
        } catch (error: unknown) {
          setFormError(
            t("settingsJsonInvalid", { message: toErrorMessage(error) })
          )
          return
        }
      }
      payload.settingsJson = settings
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
      setDrafts((prev) => ({ ...prev, [saved.id]: draftFromProfile(saved) }))
      setSelectedId(saved.id)
      toast.success(t("saveSuccess"))
    } catch (error: unknown) {
      setFormError(toErrorMessage(error) || t("saveFailed"))
    } finally {
      setSaving(false)
    }
  }, [drafts, selectedId, t])

  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return
    setDeleting(true)
    try {
      const id = deleteTarget.id
      await claudeProfileDelete(id)
      setProfiles((prev) => prev.filter((item) => item.id !== id))
      setDrafts((prev) => {
        const next = { ...prev }
        delete next[id]
        return next
      })
      setSelectedId(FOLLOW_DEFAULT_CLAUDE_PROFILE_ID)
      toast.success(t("deleteSuccess"))
      setDeleteTarget(null)
    } catch (error: unknown) {
      toast.error(t("deleteFailed"), { description: toErrorMessage(error) })
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

  return (
    <div className="space-y-2">
      <div className="min-w-0">
        <label className="text-xs font-medium">{t("sectionTitle")}</label>
        <p className="mt-1 text-[11px] text-muted-foreground">
          {t("sectionDescription")}
        </p>
      </div>

      {loading ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("loading")}
        </div>
      ) : (
        <div className="rounded-md border bg-background/50">
          {/* Profiles are tabs across the panel, the way an editor switches
              between User / Workspace settings — the fields below belong to
              whichever tab is active. No modal: a profile is a place you go,
              not a form you summon. */}
          <div
            role="tablist"
            aria-label={t("sectionTitle")}
            className="flex items-center gap-1 overflow-x-auto overflow-y-hidden border-b px-1.5 py-1.5 scrollbar-thin"
          >
            {tabs.map((tab) => {
              const active = tab.id === selectedId
              const draft = drafts[tab.id]
              const unsaved = draft
                ? isDirty(draft, profileById.get(tab.id))
                : false
              return (
                <button
                  key={tab.id}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => {
                    setFormError(null)
                    if (tab.id !== selectedId) setCopiedFrom(null)
                    setSelectedId(tab.id)
                  }}
                  className={cn(
                    "flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors",
                    active
                      ? "bg-muted font-medium text-foreground"
                      : "text-muted-foreground hover:bg-muted/60"
                  )}
                >
                  <span className="max-w-[10rem] truncate">{tab.label}</span>
                  {tab.id === currentDefault ? (
                    <Star className="h-3 w-3 fill-current opacity-70" />
                  ) : null}
                  {unsaved ? (
                    <span
                      className="h-1.5 w-1.5 rounded-full bg-primary"
                      title={t("unsaved")}
                      aria-label={t("unsaved")}
                    />
                  ) : null}
                </button>
              )
            })}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  size="icon-xs"
                  variant="ghost"
                  className="shrink-0"
                  title={t("addProfile")}
                  aria-label={t("addProfile")}
                >
                  <Plus className="h-3.5 w-3.5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="min-w-44">
                {selectedDraft ? (
                  <DropdownMenuItem onSelect={() => addProfile(selectedDraft)}>
                    <Copy className="h-3.5 w-3.5" />
                    {t("addCopy", { name: selectedDraft.label })}
                  </DropdownMenuItem>
                ) : null}
                <DropdownMenuItem onSelect={() => addProfile()}>
                  <Plus className="h-3.5 w-3.5" />
                  {t("addBlank")}
                </DropdownMenuItem>
                {/* The answer to "aren't codeg's settings just a migrated
                    settings.json?" — yes, so let the user say so in one
                    click. One-way: the source file is never written back. */}
                <DropdownMenuItem
                  disabled={importing}
                  onSelect={(event) => {
                    event.preventDefault()
                    void importFromSettings()
                  }}
                >
                  {importing ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <FileDown className="h-3.5 w-3.5" />
                  )}
                  {t("addImport")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <div className="space-y-3 p-3">
            {isVirtualTab ? (
              <p className="text-[11px] text-muted-foreground">
                {isFollowDefaultTab
                  ? t("followDefaultBody")
                  : t("officialDirectBody")}
              </p>
            ) : selectedDraft ? (
              <>
                <div className="space-y-1.5">
                  <label
                    htmlFor="claude-profile-label"
                    className="text-[11px] text-muted-foreground"
                  >
                    {t("fieldLabel")}
                  </label>
                  <Input
                    id="claude-profile-label"
                    value={selectedDraft.label}
                    onChange={(event) =>
                      patchDraft({ label: event.target.value })
                    }
                  />
                </div>

                <div className="space-y-1.5">
                  <label
                    htmlFor="claude-profile-id"
                    className="text-[11px] text-muted-foreground"
                  >
                    {t("fieldId")}
                  </label>
                  <Input
                    id="claude-profile-id"
                    value={selectedDraft.id}
                    readOnly={!selectedDraft.isNew}
                    onChange={(event) => patchDraft({ id: event.target.value })}
                  />
                  {/* The id is not decoration: it is what an agent passes to
                      `create_work_task(profile: …)`. */}
                  <p className="text-[10px] text-muted-foreground">
                    {t("idHint")}
                  </p>
                </div>

                {/* No kind picker. There are two things a user needs — follow
                    the CLI, or a config codeg owns — and a third choice
                    ("point at a directory someone else maintains") only made
                    the panel harder to read. The backend still resolves
                    `configDir` profiles, so one created through the API keeps
                    working and stays editable here; the panel just will not
                    make a new one. */}
                {selectedDraft.kind === "configDir" ? (
                  <div className="space-y-1.5">
                    <label
                      htmlFor="claude-profile-config-dir"
                      className="text-[11px] text-muted-foreground"
                    >
                      {t("fieldConfigDir")}
                    </label>
                    <Input
                      id="claude-profile-config-dir"
                      value={selectedDraft.configDir}
                      onChange={(event) =>
                        patchDraft({ configDir: event.target.value })
                      }
                      placeholder="/home/user/.claude-work"
                    />
                  </div>
                ) : (
                  <>
                    <div className="space-y-1.5">
                      <label
                        htmlFor="claude-profile-base-url"
                        className="text-[11px] text-muted-foreground"
                      >
                        {t("fieldBaseUrl")}
                      </label>
                      <Input
                        id="claude-profile-base-url"
                        value={selectedDraft.baseUrl}
                        onChange={(event) =>
                          patchDraft({ baseUrl: event.target.value })
                        }
                        placeholder="https://api.example.com"
                      />
                    </div>
                    <div className="space-y-1.5">
                      <label
                        htmlFor="claude-profile-token"
                        className="text-[11px] text-muted-foreground"
                      >
                        {t("fieldAuthToken")}
                      </label>
                      <Input
                        id="claude-profile-token"
                        type="password"
                        value={selectedDraft.authToken}
                        onChange={(event) =>
                          patchDraft({ authToken: event.target.value })
                        }
                        placeholder={
                          selectedProfile?.authTokenMasked?.trim()
                            ? t("tokenKeepPlaceholder")
                            : undefined
                        }
                        aria-describedby={
                          selectedDraft.isNew && copiedFrom
                            ? "claude-profile-token-hint"
                            : undefined
                        }
                        autoComplete="off"
                      />
                      {selectedDraft.isNew && copiedFrom ? (
                        <p
                          id="claude-profile-token-hint"
                          className="text-[10px] text-muted-foreground"
                        >
                          {t("tokenNotCopied")}
                        </p>
                      ) : null}
                    </div>
                    <div className="space-y-1.5">
                      <label
                        htmlFor="claude-profile-model"
                        className="text-[11px] text-muted-foreground"
                      >
                        {t("fieldModel")}
                      </label>
                      <Input
                        id="claude-profile-model"
                        value={selectedDraft.model}
                        onChange={(event) =>
                          patchDraft({ model: event.target.value })
                        }
                      />
                    </div>

                    {/* Everything else a profile can carry — the rest of the
                        `env` block, effortLevel, permissions, hooks — is just
                        settings.json, so it gets one editor instead of a
                        field per key. Collapsed: the three inputs above are
                        the whole job for a gateway. */}
                    <div className="space-y-1.5">
                      <button
                        type="button"
                        className="flex w-full items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
                        aria-expanded={settingsOpen}
                        onClick={() => setSettingsOpen((open) => !open)}
                      >
                        <ChevronRight
                          className={cn(
                            "h-3 w-3 transition-transform",
                            settingsOpen && "rotate-90"
                          )}
                        />
                        {t("fieldSettingsJson")}
                      </button>
                      {settingsOpen ? (
                        <>
                          <p className="text-[10px] text-muted-foreground">
                            {t("settingsJsonHint")}
                          </p>
                          {droppedSecrets.length > 0 ? (
                            <p className="text-[10px] text-amber-600 dark:text-amber-500">
                              {t("importDroppedSecrets", {
                                keys: droppedSecrets.join(", "),
                              })}
                            </p>
                          ) : null}
                          <Textarea
                            aria-label={t("fieldSettingsJson")}
                            value={selectedDraft.settingsJson}
                            onChange={(event) =>
                              patchDraft({ settingsJson: event.target.value })
                            }
                            placeholder={'{\n  "env": {}\n}'}
                            className="min-h-40 font-mono text-xs"
                            spellCheck={false}
                          />
                        </>
                      ) : null}
                    </div>
                  </>
                )}

                {formError ? (
                  <p className="text-xs text-destructive">{formError}</p>
                ) : null}
              </>
            ) : null}

            <div className="flex flex-wrap items-center gap-1.5 border-t pt-2.5">
              {selectedId === currentDefault ? (
                <Badge variant="secondary">{t("currentDefault")}</Badge>
              ) : (
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  disabled={
                    settingDefaultId === selectedId ||
                    (selectedDraft?.isNew ?? false)
                  }
                  onClick={() => void handleSetDefault(selectedId)}
                >
                  {settingDefaultId === selectedId ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Star className="h-3.5 w-3.5" />
                  )}
                  {t("setAsDefault")}
                </Button>
              )}
              <div className="flex-1" />
              {!isVirtualTab && selectedDraft?.isNew ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  onClick={discardNew}
                  disabled={saving}
                >
                  {tActions("cancel")}
                </Button>
              ) : null}
              {!isVirtualTab && selectedProfile ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => setDeleteTarget(selectedProfile)}
                  title={t("delete")}
                  aria-label={t("delete")}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              ) : null}
              {!isVirtualTab ? (
                <Button
                  type="button"
                  size="xs"
                  onClick={() => void handleSave()}
                  disabled={saving || !dirty}
                >
                  {saving ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : null}
                  {t("save")}
                </Button>
              ) : null}
            </div>
          </div>
        </div>
      )}

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

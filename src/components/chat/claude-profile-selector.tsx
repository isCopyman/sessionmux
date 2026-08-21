"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import { ChevronDown } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

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
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { DropdownRadioItemContent } from "@/components/chat/dropdown-radio-item-content"
import {
  claudeProfileList,
  conversationGetClaudeProfile,
  conversationGetProjectSettings,
  conversationSetClaudeProfile,
  conversationSetProjectSettings,
  openSettingsWindow,
} from "@/lib/api"
import { useConnection } from "@/hooks/use-connection"
import { toErrorMessage } from "@/lib/app-error"
import {
  FOLLOW_DEFAULT_CLAUDE_PROFILE_ID,
  type ClaudeProfileInfo,
} from "@/lib/types"

interface InlineClaudeProfileSelectorProps {
  conversationId: number | null
  /**
   * Connection key for this composer, so switching can restart the session it
   * belongs to. Omitted (tests, surfaces with no live connection) means the
   * switch is stored and applies on the next launch, same as before.
   */
  tabId?: string | null
  disabled?: boolean
}

function profileDescription(profile: ClaudeProfileInfo): string | null {
  if (profile.kind === "configDir") {
    return profile.configDir?.trim() || null
  }
  if (profile.kind === "managed") {
    return profile.baseUrl?.trim() || null
  }
  return null
}

export function InlineClaudeProfileSelector({
  conversationId,
  tabId = null,
  disabled = false,
}: InlineClaudeProfileSelectorProps) {
  const t = useTranslations("AcpAgentSettings.claudeProfile")
  const [profiles, setProfiles] = useState<ClaudeProfileInfo[]>([])
  const [selectedId, setSelectedId] = useState(FOLLOW_DEFAULT_CLAUDE_PROFILE_ID)
  // A switch only reaches the agent at spawn, so applying one means restarting
  // the session. Held until the user answers when a turn is in flight.
  const [confirmRestart, setConfirmRestart] = useState(false)
  const [restarting, setRestarting] = useState(false)
  // Whether this session also loads the working folder's own `.claude`
  // settings. Absent on the backend means on, so `true` is the right value to
  // show before the read lands.
  const [projectSettings, setProjectSettings] = useState(true)
  const { status, reapplyConfig } = useConnection(tabId ?? "")

  useEffect(() => {
    let cancelled = false
    void claudeProfileList()
      .then((list) => {
        if (cancelled) return
        setProfiles(list)
        setSelectedId((current) => {
          if (list.some((item) => item.id === current)) return current
          return list[0]?.id ?? FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
        })
      })
      .catch((error: unknown) => {
        if (cancelled) return
        toast.error(t("listFailed"), {
          description: toErrorMessage(error),
        })
      })
    return () => {
      cancelled = true
    }
  }, [t])

  // Which profile this conversation is actually bound to. Without this the chip
  // read `follow-default` on every mount, so reopening the app made every
  // session claim to follow the CLI no matter which endpoint it was billing.
  // A failure here is silent on purpose: the chip falls back to the list's
  // first entry exactly as it did before, and a toast for a label that is only
  // cosmetically wrong would be worse than the wrong label.
  useEffect(() => {
    if (conversationId == null) return
    let cancelled = false
    void conversationGetClaudeProfile(conversationId)
      .then((result) => {
        if (cancelled || !result.profileId) return
        setSelectedId(result.profileId)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [conversationId])

  // Same deal for the project-settings switch, and silent for the same reason.
  useEffect(() => {
    if (conversationId == null) return
    let cancelled = false
    void conversationGetProjectSettings(conversationId)
      .then((result) => {
        if (cancelled) return
        setProjectSettings(result.enabled)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [conversationId])

  const selected = useMemo(
    () => profiles.find((item) => item.id === selectedId),
    [profiles, selectedId]
  )
  const currentLabel =
    selectedId === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
      ? t("followDefault")
      : (selected?.label ?? selectedId)
  const controlName = t("controlName")

  /**
   * Restart so the running process picks the profile up. `reapplyConfig`
   * disconnects and resumes the same session, so the transcript survives and
   * the persisted prompt queue is untouched — the only casualty is a turn that
   * is mid-flight, which is why that is the one case we ask about.
   */
  const applyNow = useCallback(async () => {
    setRestarting(true)
    try {
      const restarted = await reapplyConfig()
      toast.success(restarted ? t("switchApplied") : t("switchSuccess"))
    } catch (error: unknown) {
      // The binding is already stored; only the restart failed. Say so, and
      // leave the stale-config banner to offer the retry.
      toast.error(t("switchRestartFailed"), {
        description: toErrorMessage(error),
      })
    } finally {
      setRestarting(false)
    }
  }, [reapplyConfig, t])

  /**
   * Everything in this menu is read at spawn, so every change here lands the
   * same way: nothing running → next launch picks it up; a turn in flight →
   * ask, because that turn is what a restart costs; otherwise → just restart.
   */
  const settleChange = useCallback(
    async (affectedRunningSessions: number) => {
      if (affectedRunningSessions < 1) {
        toast.success(t("switchSuccess"))
        return
      }
      if (status === "prompting") {
        setConfirmRestart(true)
        return
      }
      await applyNow()
    },
    [applyNow, status, t]
  )

  const handleSelect = useCallback(
    async (profileId: string) => {
      if (disabled || conversationId == null || profileId === selectedId) {
        return
      }
      const previous = selectedId
      setSelectedId(profileId)
      try {
        const result = await conversationSetClaudeProfile(
          conversationId,
          profileId
        )
        await settleChange(result.affectedRunningSessions)
      } catch (error: unknown) {
        setSelectedId(previous)
        toast.error(t("switchFailed"), {
          description: toErrorMessage(error),
        })
      }
    },
    [conversationId, disabled, selectedId, settleChange, t]
  )

  const handleProjectSettingsChange = useCallback(
    async (enabled: boolean) => {
      if (disabled || conversationId == null || enabled === projectSettings) {
        return
      }
      const previous = projectSettings
      setProjectSettings(enabled)
      try {
        const result = await conversationSetProjectSettings(
          conversationId,
          enabled
        )
        await settleChange(result.affectedRunningSessions)
      } catch (error: unknown) {
        setProjectSettings(previous)
        toast.error(t("switchFailed"), {
          description: toErrorMessage(error),
        })
      }
    },
    [conversationId, disabled, projectSettings, settleChange, t]
  )

  const handleManage = useCallback(() => {
    void openSettingsWindow("agents", { agentType: "claude_code" }).catch(
      (error: unknown) => {
        toast.error(t("openSettingsFailed"), {
          description: toErrorMessage(error),
        })
      }
    )
  }, [t])

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="xs"
            disabled={disabled}
            title={controlName}
            aria-label={
              currentLabel ? `${controlName}: ${currentLabel}` : controlName
            }
            className="min-w-0 gap-0.5 px-1 text-muted-foreground"
          >
            <span className="max-w-[10rem] truncate">{currentLabel}</span>
            <ChevronDown className="size-3 shrink-0 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          side="top"
          align="start"
          className="min-w-72 overflow-y-auto"
          style={{
            maxWidth: "min(20rem, calc(100vw - 1rem))",
            maxHeight:
              "min(60vh, var(--radix-dropdown-menu-content-available-height))",
          }}
        >
          <DropdownMenuLabel className="text-foreground">
            {controlName}
          </DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuRadioGroup
            value={selectedId}
            onValueChange={(value) => {
              void handleSelect(value)
            }}
          >
            {profiles.map((profile) => (
              <DropdownMenuRadioItem
                key={profile.id}
                value={profile.id}
                title={
                  profile.id === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
                    ? t("followDefault")
                    : profile.label
                }
              >
                <DropdownRadioItemContent
                  label={
                    profile.id === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
                      ? t("followDefault")
                      : profile.label
                  }
                  description={profileDescription(profile)}
                  truncateDescription
                />
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
          <DropdownMenuSeparator />
          {/* The layer below the profile, not a second profile: whatever the
            working folder ships in `.claude/`. Kept in this menu because it
            answers the same question the profile does — which config is this
            session running on. */}
          <DropdownMenuCheckboxItem
            checked={projectSettings}
            disabled={disabled}
            title={t("projectSettingsHint")}
            onSelect={(event) => {
              // Keep the menu open: this is a setting, not a destination, and
              // the next thing to read is the hint right under it.
              event.preventDefault()
            }}
            onCheckedChange={(checked) => {
              void handleProjectSettingsChange(checked === true)
            }}
          >
            {t("projectSettings")}
          </DropdownMenuCheckboxItem>
          <p className="px-2 pb-1 text-xs text-muted-foreground">
            {t("projectSettingsHint")}
          </p>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={handleManage}>
            {t("manage")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {/* Asked only while a turn is in flight — that turn is the one thing a
        restart destroys. An idle session restarts without a prompt, because a
        dialog guarding nothing is the kind of click this panel had too many
        of. */}
      <AlertDialog open={confirmRestart} onOpenChange={setConfirmRestart}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("switchRestartTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("switchRestartBody")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={restarting}>
              {t("switchRestartLater")}
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={restarting}
              onClick={() => {
                void applyNow()
              }}
            >
              {t("switchRestartConfirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

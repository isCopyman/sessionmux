"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import { ChevronDown } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
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
  conversationSetClaudeProfile,
  openSettingsWindow,
} from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import {
  FOLLOW_DEFAULT_CLAUDE_PROFILE_ID,
  type ClaudeProfileInfo,
} from "@/lib/types"

interface InlineClaudeProfileSelectorProps {
  conversationId: number | null
  disabled?: boolean
  /** Composer's pending choice while no conversation exists. Shown the same
   *  way a saved binding is. Ignored once `conversationId` is set. */
  pendingProfileId?: string | null
  onPendingProfileChange?: (profileId: string) => void
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
  disabled = false,
  pendingProfileId = null,
  onPendingProfileChange,
}: InlineClaudeProfileSelectorProps) {
  const t = useTranslations("AcpAgentSettings.claudeProfile")
  const [profiles, setProfiles] = useState<ClaudeProfileInfo[]>([])
  const [selectedId, setSelectedId] = useState(
    pendingProfileId ?? FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
  )
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

  const selected = useMemo(
    () => profiles.find((item) => item.id === selectedId),
    [profiles, selectedId]
  )
  const currentLabel =
    selectedId === FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
      ? t("followDefault")
      : (selected?.label ?? selectedId)
  const controlName = t("controlName")

  const handleSelect = useCallback(
    async (profileId: string) => {
      if (disabled || profileId === selectedId) {
        return
      }
      if (conversationId == null) {
        setSelectedId(profileId)
        onPendingProfileChange?.(profileId)
        return
      }
      const previous = selectedId
      setSelectedId(profileId)
      try {
        const result = await conversationSetClaudeProfile(
          conversationId,
          profileId
        )
        // Nothing running to restart: the next launch reads the new binding.
        if (result.affectedRunningSessions < 1) {
          toast.success(t("switchSuccess"))
          return
        }
        // A live session is restarted by SessionConfigStaleBanner, which owns
        // the turn/queue decision and reports success once the new process is up.
      } catch (error: unknown) {
        setSelectedId(previous)
        toast.error(t("switchFailed"), {
          description: toErrorMessage(error),
        })
      }
    },
    [conversationId, disabled, onPendingProfileChange, selectedId, t]
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
          <DropdownMenuItem onSelect={handleManage}>
            {t("manage")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}

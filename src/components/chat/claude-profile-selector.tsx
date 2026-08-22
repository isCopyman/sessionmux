"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
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
  getAgentProfileAdapter,
  type AgentLaunchProfileInfo,
} from "@/lib/agent-profile"
import { toErrorMessage } from "@/lib/app-error"
import { FOLLOW_DEFAULT_AGENT_PROFILE_ID, type AgentType } from "@/lib/types"

export interface AgentProfileSelectorProps {
  /** The Harness whose launch-profile adapter owns this selector. */
  agentType?: AgentType
  /** Skip profile I/O when the surrounding composer has no profile capability. */
  enabled?: boolean
  conversationId: number | null
  disabled?: boolean
  /** Composer's pending choice while no conversation exists. Shown the same
   *  way a saved binding is. Ignored once `conversationId` is set. */
  pendingProfileId?: string | null
  onPendingProfileChange?: (profileId: string) => Promise<boolean>
  /** Agent-level launch profile used by the process before a conversation row
   *  exists. `undefined` means the agent list is still loading. */
  agentDefaultProfileId?: string | null
}

/** Compatibility name for existing call sites while the selector is generic. */
export type ClaudeProfileSelectorProps = AgentProfileSelectorProps

export interface AgentProfileSelectorOption {
  value: string
  label: string
  description: string | null
}
export type ClaudeProfileSelectorOption = AgentProfileSelectorOption

/**
 * Shared state and actions for every Harness Profile selector surface.
 *
 * The wide composer uses a dropdown while the narrow composer uses the same
 * master-detail panel as Model/Mode/Effort. Keeping the profile lifecycle here
 * prevents those two responsive layouts from drifting into different fields
 * or switching semantics again.
 */
export interface AgentProfileSelectorModel {
  controlName: string
  currentLabel: string
  displayedId: string | null
  options: AgentProfileSelectorOption[]
  loading: boolean
  disabled: boolean
  select: (profileId: string) => Promise<void>
  manage: () => void
}
export type ClaudeProfileSelectorModel = AgentProfileSelectorModel

export function useAgentProfileSelectorModel({
  agentType = "claude_code",
  enabled = true,
  conversationId,
  disabled = false,
  pendingProfileId = null,
  onPendingProfileChange,
  agentDefaultProfileId,
}: AgentProfileSelectorProps): AgentProfileSelectorModel {
  const t = useTranslations("AcpAgentSettings.claudeProfile")
  const adapter = useMemo(() => getAgentProfileAdapter(agentType), [agentType])
  const profileEnabled = enabled && adapter != null
  const tRef = useRef(t)
  tRef.current = t
  const [profiles, setProfiles] = useState<AgentLaunchProfileInfo[]>([])
  const [profilesReady, setProfilesReady] = useState(false)
  const [pickedId, setPickedId] = useState<string | null>(null)
  const [switching, setSwitching] = useState(false)
  const [boundFetch, setBoundFetch] = useState<{
    conversationId: number
    profileId: string | null
  } | null>(null)
  useEffect(() => {
    if (!profileEnabled || !adapter) {
      setProfiles([])
      setProfilesReady(true)
      return
    }
    let cancelled = false
    setProfilesReady(false)
    void adapter
      .list()
      .then((list) => {
        if (cancelled) return
        setProfiles(list)
        setProfilesReady(true)
      })
      .catch((error: unknown) => {
        if (cancelled) return
        setProfilesReady(true)
        toast.error(tRef.current("listFailed"), {
          description: toErrorMessage(error),
        })
      })
    return () => {
      cancelled = true
    }
  }, [adapter, profileEnabled])

  // Which profile this conversation is actually bound to. Without this the chip
  // read `follow-default` on every mount, so reopening the app made every
  // session claim to follow the CLI no matter which endpoint it was billing.
  // A failure here is silent on purpose: the chip falls back to the list's
  // first entry exactly as it did before, and a toast for a label that is only
  // cosmetically wrong would be worse than the wrong label.
  useEffect(() => {
    if (!profileEnabled || !adapter || conversationId == null) return
    let cancelled = false
    void adapter
      .getConversationProfile(conversationId)
      .then((result) => {
        if (cancelled) return
        setBoundFetch({
          conversationId,
          profileId: result.profileId || null,
        })
      })
      .catch(() => {
        if (cancelled) return
        setBoundFetch({ conversationId, profileId: null })
      })
    return () => {
      cancelled = true
    }
  }, [adapter, conversationId, profileEnabled])

  const displayedId = useMemo(() => {
    if (!profileEnabled) return null
    if (!profilesReady) return null
    const normalize = (id: string) =>
      profiles.some((item) => item.id === id)
        ? id
        : FOLLOW_DEFAULT_AGENT_PROFILE_ID
    if (conversationId == null) {
      const chosen = pendingProfileId ?? pickedId
      if (chosen) return normalize(chosen)
      if (agentDefaultProfileId === undefined) return null
      return normalize(agentDefaultProfileId ?? FOLLOW_DEFAULT_AGENT_PROFILE_ID)
    }
    if (pickedId) return normalize(pickedId)
    if (boundFetch?.conversationId !== conversationId) return null
    return normalize(boundFetch.profileId ?? FOLLOW_DEFAULT_AGENT_PROFILE_ID)
  }, [
    agentDefaultProfileId,
    boundFetch,
    conversationId,
    pendingProfileId,
    pickedId,
    profiles,
    profilesReady,
    profileEnabled,
  ])
  const selected = useMemo(
    () => profiles.find((item) => item.id === displayedId),
    [profiles, displayedId]
  )
  const currentLabel =
    displayedId == null
      ? t("loading")
      : displayedId === FOLLOW_DEFAULT_AGENT_PROFILE_ID
        ? t("followDefault")
        : (selected?.label ?? displayedId)
  const controlName = t("controlName")
  const triggerDisabled = disabled || switching || displayedId == null

  const handleSelect = useCallback(
    async (profileId: string) => {
      if (triggerDisabled || profileId === displayedId) {
        return
      }
      if (conversationId == null) {
        if (!onPendingProfileChange) return
        setSwitching(true)
        try {
          const applied = await onPendingProfileChange(profileId)
          if (!applied) throw new Error(t("switchFailed"))
          setPickedId(profileId)
        } catch (error: unknown) {
          toast.error(t("switchFailed"), {
            description: toErrorMessage(error),
          })
        } finally {
          setSwitching(false)
        }
        return
      }
      const previous = pickedId
      try {
        if (!adapter) return
        const result = await adapter.setConversationProfile(
          conversationId,
          profileId
        )
        setPickedId(profileId)
        // Nothing running to restart: the next launch reads the new binding.
        if (result.affectedRunningSessions < 1) {
          toast.success(t("switchSuccess"))
          return
        }
        // A live session is restarted by SessionConfigStaleBanner, which owns
        // the turn/queue decision and reports success once the new process is up.
      } catch (error: unknown) {
        setPickedId(previous)
        toast.error(t("switchFailed"), {
          description: toErrorMessage(error),
        })
      }
    },
    [
      conversationId,
      adapter,
      displayedId,
      onPendingProfileChange,
      pickedId,
      t,
      triggerDisabled,
    ]
  )

  const handleManage = useCallback(() => {
    if (!adapter) return
    void adapter.manage().catch((error: unknown) => {
      toast.error(t("openSettingsFailed"), {
        description: toErrorMessage(error),
      })
    })
  }, [adapter, t])

  const options = useMemo<AgentProfileSelectorOption[]>(
    () =>
      profiles.map((profile) => ({
        value: profile.id,
        label:
          profile.id === FOLLOW_DEFAULT_AGENT_PROFILE_ID
            ? t("followDefault")
            : profile.label,
        description: profile.description,
      })),
    [profiles, t]
  )

  return useMemo(
    () => ({
      controlName,
      currentLabel,
      displayedId,
      options,
      loading: profileEnabled && !profilesReady,
      disabled: triggerDisabled,
      select: handleSelect,
      manage: handleManage,
    }),
    [
      controlName,
      currentLabel,
      displayedId,
      profileEnabled,
      handleManage,
      handleSelect,
      options,
      profilesReady,
      triggerDisabled,
    ]
  )
}

/** Compatibility hook; all lifecycle logic now lives in the generic model. */
export const useClaudeProfileSelectorModel = useAgentProfileSelectorModel

export function AgentProfileSelectorDropdown({
  model,
}: {
  model: AgentProfileSelectorModel
}) {
  const t = useTranslations("AcpAgentSettings.claudeProfile")

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="xs"
            disabled={model.disabled}
            title={model.controlName}
            aria-label={
              model.currentLabel
                ? `${model.controlName}: ${model.currentLabel}`
                : model.controlName
            }
            className="min-w-0 gap-0.5 px-1 text-muted-foreground"
          >
            <span className="max-w-[10rem] truncate">{model.currentLabel}</span>
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
            {model.controlName}
          </DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuRadioGroup
            value={model.displayedId ?? ""}
            onValueChange={(value) => {
              void model.select(value)
            }}
          >
            {model.options.map((option) => (
              <DropdownMenuRadioItem
                key={option.value}
                value={option.value}
                title={option.label}
              >
                <DropdownRadioItemContent
                  label={option.label}
                  description={option.description}
                  truncateDescription
                />
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={model.manage}>
            {t("manage")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}

export function InlineClaudeProfileSelector(props: ClaudeProfileSelectorProps) {
  const model = useAgentProfileSelectorModel(props)
  return <AgentProfileSelectorDropdown model={model} />
}

export function InlineAgentProfileSelector(props: AgentProfileSelectorProps) {
  const model = useAgentProfileSelectorModel(props)
  return <AgentProfileSelectorDropdown model={model} />
}

/** Compatibility export for older callers. */
export const ClaudeProfileSelectorDropdown = AgentProfileSelectorDropdown

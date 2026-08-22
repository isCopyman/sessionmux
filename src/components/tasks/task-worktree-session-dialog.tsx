"use client"

import { useEffect, useMemo, useState } from "react"
import { useTranslations } from "next-intl"

import {
  AgentConfigSection,
  effectiveSelections,
  snapshotLabels,
} from "@/components/automations/agent-config-section"
import { useAgentOptions } from "@/components/automations/use-agent-options"
import { AgentSelector } from "@/components/chat/agent-selector"
import { InlineClaudeProfileSelector } from "@/components/chat/claude-profile-selector"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { toErrorMessage } from "@/lib/app-error"
import { workTaskSettingsEffective } from "@/lib/api"
import { getAgentLabel } from "@/lib/custom-agents"
import {
  CODEG_CLAUDE_PROFILE_CONFIG_KEY,
  CODEG_CLAUDE_PROFILE_ENV_KEY,
  FOLLOW_DEFAULT_CLAUDE_PROFILE_ID,
  type AcpAgentInfo,
  type AgentType,
  type WorkTask,
  type WorkTaskConfig,
} from "@/lib/types"

interface TaskSessionLaunchDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  task: WorkTask | null
  folderPath: string | null
  kind: "regular" | "worktree"
  onSubmit: (config: WorkTaskConfig) => Promise<void>
}

/**
 * The explicit execution decision for a neutral task card. The task editor
 * captures only "what"; this dialog captures "which new Worktree Session".
 * It deliberately reuses the same Agent/Profile/options components as normal
 * Session creation instead of growing a task-specific ACP form.
 */
export function TaskSessionLaunchDialog({
  open,
  onOpenChange,
  task,
  folderPath,
  kind,
  onSubmit,
}: TaskSessionLaunchDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open && task ? (
        <TaskWorktreeSessionBody
          task={task}
          folderPath={folderPath}
          kind={kind}
          onSubmit={onSubmit}
          onCancel={() => onOpenChange(false)}
        />
      ) : null}
    </Dialog>
  )
}

function TaskWorktreeSessionBody({
  task,
  folderPath,
  kind,
  onSubmit,
  onCancel,
}: {
  task: WorkTask
  folderPath: string | null
  kind: "regular" | "worktree"
  onSubmit: (config: WorkTaskConfig) => Promise<void>
  onCancel: () => void
}) {
  const t = useTranslations("Tasks")
  const [loaded, setLoaded] = useState(false)
  const [agentType, setAgentType] = useState<AgentType>("claude_code")
  const [modeId, setModeId] = useState<string | null>(null)
  const [configValues, setConfigValues] = useState<Record<string, string>>({})
  const [profileId, setProfileId] = useState(FOLLOW_DEFAULT_CLAUDE_PROFILE_ID)
  const [agents, setAgents] = useState<AcpAgentInfo[]>([])
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    let cancelled = false
    void workTaskSettingsEffective(task.folder_id)
      .then((settings) => {
        if (cancelled) return
        const taskConfig = task.config
        const nextAgent =
          taskConfig?.agent_type ?? settings.default_agent_type ?? "claude_code"
        const nextConfig =
          taskConfig?.agent_type != null
            ? { ...(taskConfig.config_values ?? {}) }
            : { ...(settings.config_values ?? {}) }
        setAgentType(nextAgent)
        setModeId(
          taskConfig?.agent_type != null
            ? (taskConfig.mode_id ?? null)
            : (settings.mode_id ?? null)
        )
        setProfileId(
          nextConfig[CODEG_CLAUDE_PROFILE_CONFIG_KEY] ??
            FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
        )
        delete nextConfig[CODEG_CLAUDE_PROFILE_CONFIG_KEY]
        setConfigValues(nextConfig)
        setLoaded(true)
      })
      .catch((cause) => setError(toErrorMessage(cause)))
    return () => {
      cancelled = true
    }
  }, [task])

  const agentOptions = useAgentOptions(agentType, folderPath, loaded)
  const agentDefaultProfileId = useMemo(() => {
    if (agentType !== "claude_code") return null
    const claude = agents.find((agent) => agent.agent_type === "claude_code")
    if (!claude) return undefined
    return (
      claude.env[CODEG_CLAUDE_PROFILE_ENV_KEY] ??
      FOLLOW_DEFAULT_CLAUDE_PROFILE_ID
    )
  }, [agentType, agents])

  const submit = async () => {
    if (submitting || !loaded) return
    setSubmitting(true)
    setError(null)
    try {
      const snapshot = await agentOptions.ensure()
      const effective = effectiveSelections(snapshot, modeId, configValues)
      const launchValues = { ...effective.config_values }
      if (agentType === "claude_code") {
        launchValues[CODEG_CLAUDE_PROFILE_CONFIG_KEY] = profileId
      }
      const previous = task.config
      await onSubmit({
        prompt_blocks: previous?.prompt_blocks ?? [],
        display_text: previous?.display_text ?? "",
        agent_type: agentType,
        mode_id: effective.mode_id,
        config_values: launchValues,
        label_snapshot: {
          agent_label: getAgentLabel(agentType),
          ...snapshotLabels(snapshot, effective.mode_id, launchValues),
        },
      })
    } catch (cause) {
      setError(toErrorMessage(cause))
      setSubmitting(false)
    }
  }

  return (
    <DialogContent className="sm:max-w-[36rem]">
      <DialogHeader>
        <DialogTitle>
          {t(
            kind === "worktree" ? "worktreeSessionTitle" : "regularSessionTitle"
          )}
        </DialogTitle>
        <DialogDescription>
          {t(
            kind === "worktree"
              ? "worktreeSessionDescription"
              : "regularSessionDescription",
            { title: task.title }
          )}
        </DialogDescription>
      </DialogHeader>

      <div className="space-y-3 py-1">
        <AgentSelector
          defaultAgentType={agentType}
          onSelect={(next) => {
            setAgentType(next)
            setModeId(null)
            setConfigValues({})
            setProfileId(FOLLOW_DEFAULT_CLAUDE_PROFILE_ID)
          }}
          onFallback={setAgentType}
          onAgentsLoaded={setAgents}
          disabled={!loaded || submitting}
        />
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          {agentType === "claude_code" ? (
            <InlineClaudeProfileSelector
              conversationId={null}
              pendingProfileId={profileId}
              onPendingProfileChange={async (next) => {
                setProfileId(next)
                return true
              }}
              agentDefaultProfileId={agentDefaultProfileId}
              disabled={!loaded || submitting}
            />
          ) : null}
          <AgentConfigSection
            snapshot={agentOptions.snapshot}
            loading={agentOptions.loading}
            error={agentOptions.error}
            onReload={agentOptions.reload}
            modeId={modeId}
            configValues={configValues}
            layout="inline"
            onModeChange={setModeId}
            onConfigChange={(optionId, valueId) =>
              setConfigValues((previous) => {
                const next = { ...previous }
                if (valueId == null) delete next[optionId]
                else next[optionId] = valueId
                return next
              })
            }
          />
        </div>
        <p className="text-xs leading-5 text-muted-foreground">
          {t(
            kind === "worktree" ? "worktreeSessionHint" : "regularSessionHint"
          )}
        </p>
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
      </div>

      <DialogFooter>
        <Button variant="outline" onClick={onCancel} disabled={submitting}>
          {t("cancel")}
        </Button>
        <Button
          onClick={() => void submit()}
          disabled={!loaded || submitting || agentOptions.loading}
        >
          {submitting
            ? t(
                kind === "worktree"
                  ? "worktreeSessionStarting"
                  : "regularSessionStarting"
              )
            : t(
                kind === "worktree"
                  ? "worktreeSessionStart"
                  : "regularSessionStart"
              )}
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}

/** Compatibility wrapper for the existing Worktree-specific call sites. */
export function TaskWorktreeSessionDialog(
  props: Omit<TaskSessionLaunchDialogProps, "kind">
) {
  return <TaskSessionLaunchDialog {...props} kind="worktree" />
}

import type { AcpAgentInfo, AgentType } from "@/lib/types"
import { parseCodexModelConfig } from "@/lib/types"
import type { ClaudeAuthMode } from "../claude-config-fields"
import { DEEPSEEK_PANEL_ENV_KEYS } from "../deepseek-config-panel"
import {
  envMapToText,
  findEnvValue,
  importantEnvKeysByAgent,
  parseConfigJsonText,
  parseEnvText,
  patchEnvText,
  type AgentDraft,
  type ImportantConfigKey,
  type ImportantDraftPatch,
} from "./shared"
import {
  CLAUDE_MODEL_ENV_KEYS,
  claudeAuthModeEnvPatch,
  extractImportantConfigValues,
  inferClaudeAuthMode,
} from "./claude"
import { extractGeminiImportantValues } from "./gemini"
import { extractOpenClawImportantValues } from "./openclaw"
import { extractClineImportantValues } from "./cline"
import { extractOpenCodeConfigValues } from "./opencode"
import { parseHermesConfig } from "./hermes"
import {
  CODEX_GRANULAR_DEFAULT,
  CODEX_SANDBOX_UNSET,
  codexSandboxBaselineOf,
  extractCodexImportantValues,
  inferCodexAuthMode,
  updateTomlRootBooleanKey,
  type CodexApprovalPolicyChoice,
  type CodexAuthMode,
  type CodexSandboxDraftFields,
  type CodexSandboxModeChoice,
} from "./codex"
import {
  GROK_DEFAULT_API_BACKEND,
  inferGrokMode,
  type GrokAuthMethod,
} from "./grok"

/**
 * Fold the DeepSeek panel's own env keys, as they are actually persisted, into
 * an existing draft. Everything else in the draft — other keys, and any
 * unsaved edit to them — is left exactly as it was.
 *
 * Returns the draft unchanged when nothing moved, so this never invalidates a
 * memo or restarts a render for a no-op refresh.
 */
export function rebaseDeepSeekDraft(
  draft: AgentDraft,
  agent: AcpAgentInfo
): AgentDraft {
  // Decide on the VALUES, before rewriting anything, so an unrelated refresh
  // leaves the draft object (and its text) untouched.
  //
  // Mirrors `patchEnvText`'s own rule exactly: an empty persisted value means
  // DELETE the key, so `KEY=` present in the draft while the agent has no such
  // key IS a difference — the enable switch persists the draft wholesale, and
  // an empty `DEEPSEEK_BASE_URL` is not "use the default", it is an empty
  // endpoint.
  const current = parseEnvText(draft.envText)
  const patch: Record<string, string | undefined> = {}
  let moved = false
  for (const key of DEEPSEEK_PANEL_ENV_KEYS) {
    patch[key] = agent.env[key]
    const next = (agent.env[key] ?? "").trim()
    const present = key in current
    if (next ? current[key] !== next : present) moved = true
  }
  if (!moved) return draft
  const envText = patchEnvText(draft.envText, patch)
  if (envText === draft.envText) return draft
  const keys = importantEnvKeysByAgent("deepseek")
  const merged = parseEnvText(envText)
  return {
    ...draft,
    envText,
    apiBaseUrl: findEnvValue(merged, keys.apiBaseUrl),
    apiKey: findEnvValue(merged, keys.apiKey),
    model: findEnvValue(merged, keys.model),
  }
}

export function patchImportantConfigText(
  agentType: AgentType,
  configText: string,
  patch: ImportantDraftPatch
): {
  configText: string
  recoveredFromInvalid: boolean
} {
  const parseResult = parseConfigJsonText(configText)
  const config = parseResult.error ? {} : { ...parseResult.config }

  const assignOrRemove = (key: string, value: string | undefined) => {
    const trimmed = value?.trim() ?? ""
    if (!trimmed) {
      delete config[key]
      return
    }
    config[key] = trimmed
  }

  if (agentType === "claude_code") {
    // Claude Code: write apiBaseUrl/apiKey into config.env, not root
    const env =
      typeof config.env === "object" && config.env && !Array.isArray(config.env)
        ? { ...(config.env as Record<string, unknown>) }
        : {}
    const assignEnv = (key: string, value: string | undefined) => {
      const trimmed = value?.trim() ?? ""
      if (!trimmed) {
        delete env[key]
        return
      }
      env[key] = trimmed
    }
    // Remove root-level apiBaseUrl/apiKey if present (legacy cleanup)
    delete config.apiBaseUrl
    delete config.apiKey
    assignEnv("ANTHROPIC_BASE_URL", patch.apiBaseUrl)
    assignEnv("ANTHROPIC_AUTH_TOKEN", patch.apiKey)

    assignEnv(CLAUDE_MODEL_ENV_KEYS.claudeMainModel, patch.claudeMainModel)
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeReasoningModel,
      patch.claudeReasoningModel
    )
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeDefaultHaikuModel,
      patch.claudeDefaultHaikuModel
    )
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeDefaultSonnetModel,
      patch.claudeDefaultSonnetModel
    )
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeDefaultOpusModel,
      patch.claudeDefaultOpusModel
    )
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeCustomModelOption,
      patch.claudeCustomModelOption
    )
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeCustomModelOptionName,
      patch.claudeCustomModelOptionName
    )
    assignEnv(
      CLAUDE_MODEL_ENV_KEYS.claudeCustomModelOptionDescription,
      patch.claudeCustomModelOptionDescription
    )

    if (Object.keys(env).length === 0) {
      delete config.env
    } else {
      config.env = env
    }
  } else {
    assignOrRemove("apiBaseUrl", patch.apiBaseUrl)
    assignOrRemove("apiKey", patch.apiKey)
    assignOrRemove("model", patch.model)
  }

  return {
    configText:
      Object.keys(config).length === 0 ? "" : JSON.stringify(config, null, 2),
    recoveredFromInvalid: Boolean(parseResult.error),
  }
}

export function patchEnvByImportantKey(
  agentType: AgentType,
  envText: string,
  key: ImportantConfigKey,
  value: string
): string {
  const keys = importantEnvKeysByAgent(agentType)
  if (key === "apiBaseUrl") {
    return patchEnvText(envText, { [keys.apiBaseUrl[0]]: value })
  }
  if (key === "apiKey") {
    return patchEnvText(envText, { [keys.apiKey[0]]: value })
  }
  if (key === "model") {
    return patchEnvText(envText, { [keys.model[0]]: value })
  }
  return patchEnvText(envText, { [CLAUDE_MODEL_ENV_KEYS[key]]: value })
}

export function applyImportantFieldToDraft(
  draft: AgentDraft,
  key: ImportantConfigKey,
  value: string
): AgentDraft {
  if (key === "apiBaseUrl") return { ...draft, apiBaseUrl: value }
  if (key === "apiKey") return { ...draft, apiKey: value }
  if (key === "model") return { ...draft, model: value }
  if (key === "claudeMainModel") return { ...draft, claudeMainModel: value }
  if (key === "claudeReasoningModel") {
    return { ...draft, claudeReasoningModel: value }
  }
  if (key === "claudeDefaultHaikuModel") {
    return { ...draft, claudeDefaultHaikuModel: value }
  }
  if (key === "claudeDefaultSonnetModel") {
    return { ...draft, claudeDefaultSonnetModel: value }
  }
  if (key === "claudeDefaultOpusModel") {
    return { ...draft, claudeDefaultOpusModel: value }
  }
  if (key === "claudeCustomModelOption") {
    return { ...draft, claudeCustomModelOption: value }
  }
  if (key === "claudeCustomModelOptionName") {
    return { ...draft, claudeCustomModelOptionName: value }
  }
  return { ...draft, claudeCustomModelOptionDescription: value }
}

export function buildImportantPatchFromDraft(
  draft: AgentDraft
): ImportantDraftPatch {
  return {
    apiBaseUrl: draft.apiBaseUrl,
    apiKey: draft.apiKey,
    model: draft.model,
    claudeMainModel: draft.claudeMainModel,
    claudeReasoningModel: draft.claudeReasoningModel,
    claudeDefaultHaikuModel: draft.claudeDefaultHaikuModel,
    claudeDefaultSonnetModel: draft.claudeDefaultSonnetModel,
    claudeDefaultOpusModel: draft.claudeDefaultOpusModel,
    claudeCustomModelOption: draft.claudeCustomModelOption,
    claudeCustomModelOptionName: draft.claudeCustomModelOptionName,
    claudeCustomModelOptionDescription:
      draft.claudeCustomModelOptionDescription,
  }
}

export function buildAgentDraft(agent: AcpAgentInfo): AgentDraft {
  const configText =
    typeof agent.config_json === "string" && agent.config_json.trim()
      ? agent.config_json
      : ""
  const hermesValues =
    agent.agent_type === "hermes" ? parseHermesConfig(configText) : null
  const openCodeAuthJsonText = agent.opencode_auth_json ?? ""
  const codexAuthJsonText = agent.codex_auth_json ?? ""
  const codexConfigTomlText =
    agent.agent_type === "codex"
      ? updateTomlRootBooleanKey(
          agent.codex_config_toml ?? "",
          "disable_response_storage",
          true
        )
      : (agent.codex_config_toml ?? "")
  const codexSandbox = agent.codex_sandbox_settings ?? null
  // Seeded once, then fingerprinted, so a save can tell a real control change
  // from "untouched, still whatever config.toml says".
  const codexSandboxFields: CodexSandboxDraftFields = {
    // The granular table and the string presets are mutually exclusive upstream,
    // so a present table always wins the selector.
    codexApprovalPolicy: codexSandbox?.granular
      ? "granular"
      : ((codexSandbox?.approval_policy ??
          CODEX_SANDBOX_UNSET) as CodexApprovalPolicyChoice),
    codexGranular: codexSandbox?.granular ?? CODEX_GRANULAR_DEFAULT,
    codexSandboxMode: (codexSandbox?.sandbox_mode ??
      CODEX_SANDBOX_UNSET) as CodexSandboxModeChoice,
    codexWritableRootsText: (
      codexSandbox?.workspace_write.writable_roots ?? []
    ).join("\n"),
    codexNetworkAccess: codexSandbox?.workspace_write.network_access ?? false,
    codexExcludeTmpdirEnvVar:
      codexSandbox?.workspace_write.exclude_tmpdir_env_var ?? false,
    codexExcludeSlashTmp:
      codexSandbox?.workspace_write.exclude_slash_tmp ?? false,
  }
  const grokConfigTomlText = agent.grok_config_toml ?? ""
  const grokPermissionMode = agent.grok_settings?.permission_mode ?? ""
  const grokReasoningEffort =
    agent.grok_settings?.default_reasoning_effort ?? ""
  const important = extractImportantConfigValues(
    agent.agent_type,
    agent.env,
    configText
  )
  const geminiImportant = extractGeminiImportantValues(agent.env, configText)
  const openClawImportant = extractOpenClawImportantValues(
    agent.env,
    configText
  )
  const codexImportant = extractCodexImportantValues(
    codexAuthJsonText,
    codexConfigTomlText
  )
  const openCodeImportant = extractOpenCodeConfigValues(
    configText,
    openCodeAuthJsonText
  )
  const clineImportant = extractClineImportantValues(configText)
  const codexAuthMode: CodexAuthMode =
    agent.agent_type === "codex" && agent.model_provider_id != null
      ? "model_provider"
      : agent.agent_type === "codex"
        ? inferCodexAuthMode(codexAuthJsonText)
        : "api_key"
  const grokAuthMode: GrokAuthMethod =
    agent.agent_type === "grok"
      ? inferGrokMode(
          agent.env,
          Boolean(agent.grok_settings?.custom_model_id?.trim())
        )
      : "api_key"
  const claudeAuthMode: ClaudeAuthMode =
    agent.agent_type === "claude_code"
      ? inferClaudeAuthMode(
          agent.env,
          agent.model_provider_id ?? null,
          Boolean(important.apiBaseUrl || important.apiKey)
        )
      : "official_subscription"
  const rawEnvText = envMapToText(agent.env)
  // When codex is in official subscription mode, clean up API keys/URLs from env.
  // Grok mirrors this: record the auth-method knob, and in subscription mode
  // strip XAI_API_KEY so the editable env can't override the `grok login`
  // credential (the launch path enforces the same — see apply_grok_env_policy).
  // Claude does the same for its own knob: writing the inferred mode back is what
  // upgrades a legacy row into one apply_claude_env_policy can act on, so an
  // inherited ANTHROPIC_BASE_URL stops reaching the subscription launch.
  const envText =
    agent.agent_type === "codex" && codexAuthMode === "chatgpt_subscription"
      ? patchEnvText(rawEnvText, {
          OPENAI_API_KEY: "",
          OPENAI_BASE_URL: "",
        })
      : agent.agent_type === "grok"
        ? patchEnvText(rawEnvText, {
            GROK_AUTH_MODE: grokAuthMode,
            ...(grokAuthMode === "subscription" ? { XAI_API_KEY: "" } : {}),
          })
        : agent.agent_type === "claude_code"
          ? patchEnvText(rawEnvText, claudeAuthModeEnvPatch(claudeAuthMode))
          : rawEnvText
  return {
    enabled: agent.enabled,
    envText,
    configText,
    apiBaseUrl:
      agent.agent_type === "hermes"
        ? (hermesValues?.baseUrl ?? "")
        : agent.agent_type === "codex"
          ? codexImportant.apiBaseUrl
          : agent.agent_type === "gemini"
            ? geminiImportant.apiBaseUrl
            : important.apiBaseUrl,
    apiKey:
      agent.agent_type === "hermes"
        ? (hermesValues?.apiKey ?? "")
        : agent.agent_type === "codex"
          ? (codexImportant.apiKey ?? "")
          : agent.agent_type === "gemini"
            ? geminiImportant.geminiApiKey || geminiImportant.googleApiKey
            : important.apiKey,
    model:
      agent.agent_type === "hermes"
        ? (hermesValues?.model ?? "")
        : agent.agent_type === "codex"
          ? codexImportant.model
          : agent.agent_type === "gemini"
            ? geminiImportant.model
            : agent.agent_type === "open_code"
              ? openCodeImportant.model
              : important.model,
    claudeAuthMode,
    modelProviderId: agent.model_provider_id ?? null,
    geminiAuthMode:
      agent.agent_type === "gemini" && agent.model_provider_id != null
        ? "model_provider"
        : geminiImportant.authMode,
    geminiApiKey: geminiImportant.geminiApiKey,
    googleApiKey: geminiImportant.googleApiKey,
    googleCloudProject: geminiImportant.googleCloudProject,
    googleCloudLocation: geminiImportant.googleCloudLocation,
    googleApplicationCredentials: geminiImportant.googleApplicationCredentials,
    codexAuthMode,
    codexModelProvider: codexImportant.modelProvider,
    codexProviderOptions: codexImportant.providerOptions,
    codexReasoningEffort: codexImportant.reasoningEffort,
    codexSupportsWebsockets: codexImportant.supportsWebsockets,
    codexSkills: codexImportant.skills,
    codexServiceTierFast: codexImportant.serviceTierFast,
    ...codexSandboxFields,
    codexSandboxBaseline: codexSandboxBaselineOf(codexSandboxFields),
    codexSandboxShadowed:
      codexSandbox?.shadowed_by_default_permissions ?? false,
    codexSandboxHasPermissionsTable:
      codexSandbox?.has_permissions_table ?? false,
    claudeMainModel: important.claudeMainModel,
    claudeReasoningModel: important.claudeReasoningModel,
    claudeDefaultHaikuModel: important.claudeDefaultHaikuModel,
    claudeDefaultSonnetModel: important.claudeDefaultSonnetModel,
    claudeDefaultOpusModel: important.claudeDefaultOpusModel,
    claudeCustomModelOption: important.claudeCustomModelOption,
    claudeCustomModelOptionName: important.claudeCustomModelOptionName,
    claudeCustomModelOptionDescription:
      important.claudeCustomModelOptionDescription,
    claudeEffortLevel: important.claudeEffortLevel,
    claudeSendAttributionHeader: important.claudeSendAttributionHeader,
    claudeDisableNonessentialTraffic:
      important.claudeDisableNonessentialTraffic,
    codexAuthJsonText,
    codexConfigTomlText,
    codexModelList: parseCodexModelConfig(agent.codex_model_catalog ?? null),
    grokConfigTomlText,
    grokAuthMode,
    grokPermissionMode,
    grokReasoningEffort,
    grokCustomModelId: agent.grok_settings?.custom_model_id ?? "",
    grokCustomBaseUrl: agent.grok_settings?.custom_base_url ?? "",
    grokCustomApiKey: agent.grok_settings?.custom_api_key ?? "",
    grokCustomApiBackend:
      agent.grok_settings?.custom_api_backend ?? GROK_DEFAULT_API_BACKEND,
    grokCustomContextWindow:
      agent.grok_settings?.custom_context_window != null
        ? String(agent.grok_settings.custom_context_window)
        : "",
    grokAutoCompactThreshold:
      agent.grok_settings?.auto_compact_threshold_percent != null
        ? String(agent.grok_settings.auto_compact_threshold_percent)
        : "",
    openCodeAuthJsonText,
    openClawGatewayUrl: openClawImportant.gatewayUrl,
    openClawGatewayToken: openClawImportant.gatewayToken,
    openClawSessionKey: openClawImportant.sessionKey,
    clineProvider: clineImportant.provider,
    clineApiKey: clineImportant.apiKey,
    clineModel: clineImportant.model,
    clineBaseUrl: clineImportant.baseUrl,
    hermesProvider: hermesValues?.provider ?? "openrouter",
    hermesConfigYaml: agent.hermes_config_yaml ?? "",
    hermesHome: hermesValues?.hermesHome ?? "",
    hermesSetupCommand: hermesValues?.setupCommand ?? "",
    hermesModelCommand: hermesValues?.modelCommand ?? "",
  }
}

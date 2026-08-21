import type { AgentType } from "@/lib/types"
import type { ClaudeEffortLevel } from "../claude-config-fields"
import {
  envFromConfig,
  findEnvValue,
  importantEnvKeysByAgent,
  parseConfigJsonText,
  pickFirstString,
} from "./shared"

export const CLAUDE_MODEL_ENV_KEYS = {
  claudeMainModel: "ANTHROPIC_MODEL",
  claudeReasoningModel: "ANTHROPIC_REASONING_MODEL",
  claudeDefaultHaikuModel: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  claudeDefaultSonnetModel: "ANTHROPIC_DEFAULT_SONNET_MODEL",
  claudeDefaultOpusModel: "ANTHROPIC_DEFAULT_OPUS_MODEL",
  claudeCustomModelOption: "ANTHROPIC_CUSTOM_MODEL_OPTION",
  claudeCustomModelOptionName: "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
  claudeCustomModelOptionDescription:
    "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
} as const

// Claude Code hardening flags surfaced as toggles below the reasoning settings.
// Each maps to a boolean env var in the native config's `env` (on = "1", off =
// "0"). Because Claude Code's own defaults are the opposite of what we want, the
// toggle values are materialized on save (see the config-save handler) so the
// shown default positions are actually applied — not left implicit/absent.
export const CLAUDE_ATTRIBUTION_HEADER_ENV_KEY =
  "CLAUDE_CODE_ATTRIBUTION_HEADER"
export const CLAUDE_NONESSENTIAL_TRAFFIC_ENV_KEY =
  "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"
export const CLAUDE_ENV_FLAG_ON = "1"
// `CLAUDE_CODE_ATTRIBUTION_HEADER` = "send the attribution/billing header" →
// default OFF (don't send). `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` = "disable
// telemetry / redundant pings" → default ON (disabled).
export const CLAUDE_SEND_ATTRIBUTION_HEADER_DEFAULT = false
export const CLAUDE_DISABLE_NONESSENTIAL_TRAFFIC_DEFAULT = true

export const CLAUDE_EFFORT_LEVEL_CONFIG_KEY = "effortLevel"

export function normalizeClaudeEffortLevel(value: unknown): ClaudeEffortLevel {
  if (typeof value !== "string") return ""
  const normalized = value.trim().toLowerCase()
  // Upstream claude-agent-acp >=0.37 exposes the sentinel string "default";
  // collapse it to "" so our UI's "默认/Default" placeholder stays
  // canonical regardless of which side wrote the config.
  if (normalized === "" || normalized === "default") return ""
  if (
    normalized === "low" ||
    normalized === "medium" ||
    normalized === "high" ||
    normalized === "xhigh"
  ) {
    return normalized
  }
  return ""
}

export type ClaudeModelKey = keyof typeof CLAUDE_MODEL_ENV_KEYS

export function extractImportantConfigValues(
  agentType: AgentType,
  env: Record<string, string>,
  configText: string
): {
  apiBaseUrl: string
  apiKey: string
  model: string
  claudeMainModel: string
  claudeReasoningModel: string
  claudeDefaultHaikuModel: string
  claudeDefaultSonnetModel: string
  claudeDefaultOpusModel: string
  claudeCustomModelOption: string
  claudeCustomModelOptionName: string
  claudeCustomModelOptionDescription: string
  claudeEffortLevel: ClaudeEffortLevel
  claudeSendAttributionHeader: boolean
  claudeDisableNonessentialTraffic: boolean
  configError: string | null
} {
  const parseResult = parseConfigJsonText(configText)
  const config = parseResult.config
  const keys = importantEnvKeysByAgent(agentType)

  const configEnv = envFromConfig(config)
  const mergedEnv = { ...env, ...configEnv }

  const apiBaseUrl =
    pickFirstString(config, ["apiBaseUrl", "api_base_url"]) ??
    findEnvValue(mergedEnv, keys.apiBaseUrl)
  const apiKey =
    pickFirstString(config, ["apiKey", "api_key"]) ??
    findEnvValue(mergedEnv, keys.apiKey)
  const model =
    pickFirstString(config, ["model", "model_name"]) ??
    findEnvValue(mergedEnv, keys.model)
  const claudeMainModel = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeMainModel,
  ])
  const claudeReasoningModel = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeReasoningModel,
  ])
  const claudeDefaultHaikuModel = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeDefaultHaikuModel,
  ])
  const claudeDefaultSonnetModel = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeDefaultSonnetModel,
  ])
  const claudeDefaultOpusModel = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeDefaultOpusModel,
  ])
  const claudeCustomModelOption = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeCustomModelOption,
  ])
  const claudeCustomModelOptionName = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeCustomModelOptionName,
  ])
  const claudeCustomModelOptionDescription = findEnvValue(mergedEnv, [
    CLAUDE_MODEL_ENV_KEYS.claudeCustomModelOptionDescription,
  ])

  const claudeEffortLevel: ClaudeEffortLevel =
    agentType === "claude_code"
      ? normalizeClaudeEffortLevel(config[CLAUDE_EFFORT_LEVEL_CONFIG_KEY])
      : ""

  // Present in env → on iff value is "1"; absent → the toggle's default.
  const attributionRaw = findEnvValue(mergedEnv, [
    CLAUDE_ATTRIBUTION_HEADER_ENV_KEY,
  ])
  const claudeSendAttributionHeader =
    agentType === "claude_code"
      ? attributionRaw
        ? attributionRaw === CLAUDE_ENV_FLAG_ON
        : CLAUDE_SEND_ATTRIBUTION_HEADER_DEFAULT
      : false
  const nonessentialRaw = findEnvValue(mergedEnv, [
    CLAUDE_NONESSENTIAL_TRAFFIC_ENV_KEY,
  ])
  const claudeDisableNonessentialTraffic =
    agentType === "claude_code"
      ? nonessentialRaw
        ? nonessentialRaw === CLAUDE_ENV_FLAG_ON
        : CLAUDE_DISABLE_NONESSENTIAL_TRAFFIC_DEFAULT
      : false

  return {
    apiBaseUrl: apiBaseUrl ?? "",
    apiKey: apiKey ?? "",
    model: model ?? "",
    claudeMainModel: agentType === "claude_code" ? (claudeMainModel ?? "") : "",
    claudeReasoningModel:
      agentType === "claude_code" ? claudeReasoningModel : "",
    claudeDefaultHaikuModel:
      agentType === "claude_code" ? claudeDefaultHaikuModel : "",
    claudeDefaultSonnetModel:
      agentType === "claude_code" ? claudeDefaultSonnetModel : "",
    claudeDefaultOpusModel:
      agentType === "claude_code" ? claudeDefaultOpusModel : "",
    claudeCustomModelOption:
      agentType === "claude_code" ? claudeCustomModelOption : "",
    claudeCustomModelOptionName:
      agentType === "claude_code" ? claudeCustomModelOptionName : "",
    claudeCustomModelOptionDescription:
      agentType === "claude_code" ? claudeCustomModelOptionDescription : "",
    claudeEffortLevel,
    claudeSendAttributionHeader,
    claudeDisableNonessentialTraffic,
    configError: parseResult.error,
  }
}

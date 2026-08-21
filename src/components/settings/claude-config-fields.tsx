export const CLAUDE_AUTH_MODES = [
  "inherit",
  "official_subscription",
  "custom",
  "model_provider",
] as const

export type ClaudeAuthMode = (typeof CLAUDE_AUTH_MODES)[number]

export type ClaudeEffortLevel = "" | "low" | "medium" | "high" | "xhigh"

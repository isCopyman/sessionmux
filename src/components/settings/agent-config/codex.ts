import { parse as parseTomlDocument } from "smol-toml"
import { toErrorMessage } from "@/lib/app-error"
import type {
  CodexGranularApproval,
  CodexSandboxStructuredConfig,
} from "@/lib/types"
import { acpText, pickFirstString } from "./shared"

export interface CodexTomlImportantValues {
  model: string
  modelProvider: string
  modelReasoningEffort: CodexReasoningEffort
  providerNames: string[]
  providerBaseUrls: Record<string, string>
  providerSupportsWebsockets: Record<string, boolean>
  featureResponsesWebsocketsV2: boolean
  featureSkills: boolean
  serviceTierFast: boolean
}

export interface CodexImportantValues {
  apiBaseUrl: string
  apiKey: string | null
  model: string
  modelProvider: string
  reasoningEffort: CodexReasoningEffort
  providerOptions: string[]
  supportsWebsockets: boolean
  skills: boolean
  serviceTierFast: boolean
}

export const CODEX_DEFAULT_MODEL_PROVIDER = "codeg"

/**
 * Header codex reads to decide whether a provider authenticates through the
 * "actor authorization" path. Mirrors `OPENAI_ACTOR_AUTHORIZATION_HEADER` in
 * codex's `model-provider-info` crate.
 */
export const CODEX_ACTOR_AUTHORIZATION_HEADER = "x-openai-actor-authorization"

export const CODEX_AUTH_MODES = [
  "api_key",
  "chatgpt_subscription",
  "model_provider",
] as const
export type CodexAuthMode = (typeof CODEX_AUTH_MODES)[number]

export type CodexReasoningEffort = "low" | "medium" | "high" | "xhigh"

export const CODEX_REASONING_EFFORT_OPTIONS: ReadonlyArray<{
  value: CodexReasoningEffort
  label: string
  description: string
}> = [
  {
    value: "low",
    label: "Low",
    description: "Fast responses with lighter reasoning",
  },
  {
    value: "medium",
    label: "Medium",
    description: "Balances speed and reasoning depth for everyday tasks",
  },
  {
    value: "high",
    label: "High",
    description: "Greater reasoning depth for complex problems",
  },
  {
    value: "xhigh",
    label: "Extra High",
    description: "Extra high reasoning depth for complex problems",
  },
]

export const CODEX_DEFAULT_REASONING_EFFORT: CodexReasoningEffort = "high"

/** The draft value meaning "leave the key out of config.toml", i.e. let codex
 * apply its own default. */
export const CODEX_SANDBOX_UNSET = ""

/** Radix Select rejects "" as an item value, so the unset choice travels
 * through the widget under this sentinel and is mapped back on change. */
export const CODEX_SANDBOX_UNSET_OPTION = "__codex_unset__"

/** `approval_policy` choices. The three presets are `AskForApproval`'s plain
 * string variants; `granular` is its table variant and reveals five switches.
 * (`on-failure` is only a legacy serde alias of `on-request` upstream, so it is
 * normalized away by the backend rather than offered here.) */
export const CODEX_APPROVAL_POLICY_VALUES = [
  "on-request",
  "untrusted",
  "never",
  "granular",
] as const
export type CodexApprovalPolicyChoice =
  | typeof CODEX_SANDBOX_UNSET
  | (typeof CODEX_APPROVAL_POLICY_VALUES)[number]

/** `SandboxMode`'s complete upstream vocabulary. */
export const CODEX_SANDBOX_MODE_VALUES = [
  "read-only",
  "workspace-write",
  "danger-full-access",
] as const
export type CodexSandboxModeChoice =
  | typeof CODEX_SANDBOX_UNSET
  | (typeof CODEX_SANDBOX_MODE_VALUES)[number]

/** The five `granular` flags, in the order they are shown. */
export const CODEX_GRANULAR_KEYS = [
  "sandbox_approval",
  "rules",
  "skill_approval",
  "request_permissions",
  "mcp_elicitations",
] as const

export const CODEX_GRANULAR_DEFAULT: CodexGranularApproval = {
  sandbox_approval: true,
  rules: true,
  skill_approval: false,
  request_permissions: false,
  mcp_elicitations: true,
}

/** codex resolves a RELATIVE `writable_roots` entry against `CODEX_HOME`
 * instead of rejecting it, so `docs` would silently grant write access to
 * `~/.codex/docs`. Absolute-only is enforced here (and again server-side).
 * Both POSIX and Windows shapes are accepted regardless of host, since
 * config.toml is portable. */
export function isAbsoluteWritableRoot(value: string): boolean {
  const trimmed = value.trim()
  if (trimmed.startsWith("/") || trimmed.startsWith("\\\\")) return true
  return /^[A-Za-z]:[\\/]/.test(trimmed)
}

/** One path per line → trimmed, de-blanked list. */
export function parseWritableRootsText(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

/** The first relative entry, or null when every entry is absolute. */
export function firstRelativeWritableRoot(text: string): string | null {
  return (
    parseWritableRootsText(text).find(
      (root) => !isAbsoluteWritableRoot(root)
    ) ?? null
  )
}

/** Whether the workspace-write sub-group applies. `sandbox_mode` unset falls
 * back to `workspace-write` for any directory carrying a `[projects]` trust
 * decision (which codeg writes for every folder it opens), so "unset" keeps the
 * group live rather than greying out the very knobs the fallback uses. */
export function codexWorkspaceWriteApplies(
  mode: CodexSandboxModeChoice
): boolean {
  return mode === "workspace-write" || mode === CODEX_SANDBOX_UNSET
}

/** The draft slice the sandbox payload is derived from. */
export type CodexSandboxDraftFields = {
  codexApprovalPolicy: CodexApprovalPolicyChoice
  codexGranular: CodexGranularApproval
  codexSandboxMode: CodexSandboxModeChoice
  codexWritableRootsText: string
  codexNetworkAccess: boolean
  codexExcludeTmpdirEnvVar: boolean
  codexExcludeSlashTmp: boolean
}

/** The sandbox controls as they were read off disk, kept on the draft so a save
 * can send ONLY what the user actually moved. */
export type CodexSandboxBaseline = CodexSandboxDraftFields

/** Baseline snapshot to seed a fresh draft with. */
export function codexSandboxBaselineOf(
  fields: CodexSandboxDraftFields
): CodexSandboxBaseline {
  return { ...fields }
}

/** Build the save PATCH for the Codex sandbox / approval controls: only the
 * fields whose control actually moved relative to `codexSandboxBaseline`.
 * Exported for tests.
 *
 * A whole-group payload would be wrong here. The panel sends the raw
 * config.toml text alongside this patch and the backend applies the patch LAST,
 * so any of these keys the user hand-edited in the raw editor — a surface the
 * panel never parses back into its controls — would be reverted by the panel's
 * stale value for that key. A per-field patch touches nothing the user did not
 * touch, in either surface.
 *
 * Throws on a relative `writable_roots` entry (only when that field moved) so
 * the save surfaces it instead of writing a path that would silently resolve
 * inside `~/.codex`. */
export function buildCodexSandboxConfig(
  draft: CodexSandboxDraftFields & {
    codexSandboxBaseline: CodexSandboxBaseline
  }
): CodexSandboxStructuredConfig {
  const base = draft.codexSandboxBaseline
  const patch: CodexSandboxStructuredConfig = {}

  // Approval is one externally tagged key upstream, so its two representations
  // move together: send both (one nulled) whenever either side changed.
  const granular = draft.codexApprovalPolicy === "granular"
  const approvalChanged =
    draft.codexApprovalPolicy !== base.codexApprovalPolicy ||
    (granular &&
      JSON.stringify(draft.codexGranular) !==
        JSON.stringify(base.codexGranular))
  if (approvalChanged) {
    patch.approvalPolicy =
      granular || draft.codexApprovalPolicy === CODEX_SANDBOX_UNSET
        ? null
        : draft.codexApprovalPolicy
    patch.granular = granular ? draft.codexGranular : null
  }

  if (draft.codexSandboxMode !== base.codexSandboxMode) {
    patch.sandboxMode =
      draft.codexSandboxMode === CODEX_SANDBOX_UNSET
        ? null
        : draft.codexSandboxMode
  }

  // The workspace-write group is sent as-is even in the modes that ignore it:
  // codex only reads it under `workspace-write`, so a dormant value costs
  // nothing, while clearing it would destroy the user's roots/flags on a round
  // trip through read-only or full-access.
  const roots = parseWritableRootsText(draft.codexWritableRootsText)
  const baseRoots = parseWritableRootsText(base.codexWritableRootsText)
  if (JSON.stringify(roots) !== JSON.stringify(baseRoots)) {
    const relative = roots.find((root) => !isAbsoluteWritableRoot(root))
    if (relative) {
      // `.replace` also covers the no-translator path, where acpText returns
      // the fallback uninterpolated.
      throw new Error(
        acpText(
          "codex.sandboxRootsRelativeError",
          "Writable folders must be absolute paths: {path}",
          { path: relative }
        ).replace("{path}", relative)
      )
    }
    patch.writableRoots = roots
  }
  if (draft.codexNetworkAccess !== base.codexNetworkAccess) {
    patch.networkAccess = draft.codexNetworkAccess
  }
  if (draft.codexExcludeTmpdirEnvVar !== base.codexExcludeTmpdirEnvVar) {
    patch.excludeTmpdirEnvVar = draft.codexExcludeTmpdirEnvVar
  }
  if (draft.codexExcludeSlashTmp !== base.codexExcludeSlashTmp) {
    patch.excludeSlashTmp = draft.codexExcludeSlashTmp
  }

  return patch
}

/** The `codexSandbox` value a Codex save should carry, or `undefined` when no
 * control moved (so the field is omitted from the request entirely). */
export function codexSandboxSaveConfig(
  draft: CodexSandboxDraftFields & {
    codexSandboxBaseline: CodexSandboxBaseline
  }
): CodexSandboxStructuredConfig | undefined {
  const patch = buildCodexSandboxConfig(draft)
  return Object.keys(patch).length > 0 ? patch : undefined
}

export function normalizeCodexReasoningEffort(
  value: string
): CodexReasoningEffort | null {
  const normalized = value.trim().toLowerCase()
  if (
    normalized === "low" ||
    normalized === "medium" ||
    normalized === "high" ||
    normalized === "xhigh"
  ) {
    return normalized
  }
  return null
}

export function buildCodexProviderOptions(
  activeProvider: string,
  providerNames: string[]
): string[] {
  const result: string[] = []
  const seen = new Set<string>()
  for (const raw of [
    activeProvider,
    ...providerNames,
    CODEX_DEFAULT_MODEL_PROVIDER,
  ]) {
    const provider = raw.trim()
    if (!provider || seen.has(provider)) continue
    seen.add(provider)
    result.push(provider)
  }
  return result
}

export function parseTomlStringLiteral(raw: string): string | null {
  const text = raw.trim()
  if (!text) return null

  if (text.startsWith('"')) {
    let escaped = false
    for (let i = 1; i < text.length; i += 1) {
      const ch = text[i]
      if (escaped) {
        escaped = false
        continue
      }
      if (ch === "\\") {
        escaped = true
        continue
      }
      if (ch === '"') {
        const literal = text.slice(0, i + 1)
        try {
          return JSON.parse(literal) as string
        } catch {
          return literal.slice(1, -1)
        }
      }
    }
    return null
  }

  if (text.startsWith("'")) {
    const end = text.indexOf("'", 1)
    if (end <= 0) return null
    return text.slice(1, end)
  }

  return null
}

export function parseTomlStringAssignment(
  rawLine: string
): { key: string; value: string } | null {
  const key = parseTomlAssignmentKey(rawLine)
  if (!key) return null
  const line = rawLine.trim()
  const equalsIndex = line.indexOf("=")
  const valueText = line.slice(equalsIndex + 1)
  const value = parseTomlStringLiteral(valueText)
  if (value === null) return null
  return { key, value: value.trim() }
}

export function parseTomlAssignmentKey(rawLine: string): string | null {
  const line = rawLine.trim()
  if (!line || line.startsWith("#")) return null
  const equalsIndex = line.indexOf("=")
  if (equalsIndex <= 0) return null
  const key = line.slice(0, equalsIndex).trim()
  if (!/^[A-Za-z0-9_.-]+$/.test(key)) return null
  return key
}

export function parseTomlBooleanAssignment(
  rawLine: string
): { key: string; value: boolean } | null {
  const key = parseTomlAssignmentKey(rawLine)
  if (!key) return null
  const line = rawLine.trim()
  const equalsIndex = line.indexOf("=")
  const valueText = line.slice(equalsIndex + 1).trim()
  const boolMatch = valueText.match(/^(true|false)(?:\s+#.*)?$/)
  if (!boolMatch) return null
  return { key, value: boolMatch[1] === "true" }
}

export function extractCodexTomlImportantValues(
  configTomlText: string
): CodexTomlImportantValues {
  const providerBaseUrls: Record<string, string> = {}
  const providerSupportsWebsockets: Record<string, boolean> = {}
  const providerNames = new Set<string>()
  let model = ""
  let modelProvider = ""
  let modelReasoningEffort: CodexReasoningEffort =
    CODEX_DEFAULT_REASONING_EFFORT
  let featureResponsesWebsocketsV2 = false
  let featureSkills = false
  let serviceTierFast = false
  let currentProviderSection: string | null = null
  let inFeaturesSection = false

  for (const rawLine of configTomlText.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line || line.startsWith("#")) continue

    const sectionMatch = line.match(
      /^\[\s*model_providers\.([A-Za-z0-9_-]+)\s*\]$/
    )
    if (sectionMatch) {
      currentProviderSection = sectionMatch[1]
      inFeaturesSection = false
      if (currentProviderSection.trim()) {
        providerNames.add(currentProviderSection.trim())
      }
      continue
    }
    if (line.match(/^\[\s*features\s*\]$/)) {
      inFeaturesSection = true
      currentProviderSection = null
      continue
    }
    if (line.startsWith("[") && line.endsWith("]")) {
      currentProviderSection = null
      inFeaturesSection = false
      continue
    }

    const assignment = parseTomlStringAssignment(rawLine)
    if (assignment) {
      if (assignment.key === "model") {
        model = assignment.value
        continue
      }
      if (assignment.key === "model_provider") {
        modelProvider = assignment.value
        continue
      }
      if (assignment.key === "model_reasoning_effort") {
        modelReasoningEffort =
          normalizeCodexReasoningEffort(assignment.value) ??
          CODEX_DEFAULT_REASONING_EFFORT
        continue
      }
      if (
        !currentProviderSection &&
        !inFeaturesSection &&
        assignment.key === "service_tier"
      ) {
        serviceTierFast = assignment.value.toLowerCase() === "fast"
        continue
      }
    }

    const boolAssignment = parseTomlBooleanAssignment(rawLine)
    if (boolAssignment) {
      if (
        currentProviderSection &&
        boolAssignment.key === "supports_websockets"
      ) {
        providerSupportsWebsockets[currentProviderSection] =
          boolAssignment.value
        providerNames.add(currentProviderSection.trim())
        continue
      }
      if (
        inFeaturesSection &&
        boolAssignment.key === "responses_websockets_v2"
      ) {
        featureResponsesWebsocketsV2 = boolAssignment.value
        continue
      }
      if (inFeaturesSection && boolAssignment.key === "skills") {
        featureSkills = boolAssignment.value
        continue
      }
      const dottedProviderWebsocketMatch = boolAssignment.key.match(
        /^model_providers\.([A-Za-z0-9_-]+)\.supports_websockets$/
      )
      if (dottedProviderWebsocketMatch && dottedProviderWebsocketMatch[1]) {
        const providerName = dottedProviderWebsocketMatch[1].trim()
        providerNames.add(providerName)
        providerSupportsWebsockets[providerName] = boolAssignment.value
        continue
      }
      if (boolAssignment.key === "features.responses_websockets_v2") {
        featureResponsesWebsocketsV2 = boolAssignment.value
        continue
      }
      if (boolAssignment.key === "features.skills") {
        featureSkills = boolAssignment.value
        continue
      }
    }

    if (!assignment) continue

    const rawAssignmentKey = parseTomlAssignmentKey(rawLine)
    const dottedProviderMatch = rawAssignmentKey?.match(
      /^model_providers\.([A-Za-z0-9_-]+)\./
    )
    if (dottedProviderMatch && dottedProviderMatch[1]) {
      providerNames.add(dottedProviderMatch[1].trim())
    }
    if (
      currentProviderSection &&
      assignment.key === "base_url" &&
      assignment.value
    ) {
      providerBaseUrls[currentProviderSection] = assignment.value
      providerNames.add(currentProviderSection.trim())
      continue
    }
    const dottedMatch = assignment.key.match(
      /^model_providers\.([A-Za-z0-9_-]+)\.base_url$/
    )
    if (dottedMatch && assignment.value) {
      providerBaseUrls[dottedMatch[1]] = assignment.value
      providerNames.add(dottedMatch[1].trim())
    }
  }
  if (modelProvider.trim()) {
    providerNames.add(modelProvider.trim())
  }
  providerNames.add(CODEX_DEFAULT_MODEL_PROVIDER)
  for (const providerName of Object.keys(providerBaseUrls)) {
    if (providerName.trim()) {
      providerNames.add(providerName.trim())
    }
  }

  return {
    model,
    modelProvider,
    modelReasoningEffort,
    providerNames: Array.from(providerNames),
    providerBaseUrls,
    providerSupportsWebsockets,
    featureResponsesWebsocketsV2,
    featureSkills,
    serviceTierFast,
  }
}

export function parseCodexAuthJsonObject(authJsonText: string): {
  authObject: Record<string, unknown> | null
  error: string | null
} {
  const trimmed = authJsonText.trim()
  if (!trimmed) return { authObject: {}, error: null }
  try {
    const parsed = JSON.parse(trimmed) as unknown
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {
        authObject: null,
        error: acpText(
          "errors.authMustBeObject",
          "auth.json must be a JSON object"
        ),
      }
    }
    return { authObject: parsed as Record<string, unknown>, error: null }
  } catch (err) {
    const message = toErrorMessage(err)
    return {
      authObject: null,
      error: acpText(
        "errors.authInvalid",
        "auth.json format error: {message}",
        {
          message,
        }
      ),
    }
  }
}

export function parseCodexAuthJsonText(authJsonText: string): string | null {
  return parseCodexAuthJsonObject(authJsonText).error
}

export function inferCodexAuthMode(authJsonText: string): CodexAuthMode {
  const { authObject } = parseCodexAuthJsonObject(authJsonText)
  if (authObject) {
    // 官网订阅：auth_mode 为 chatgpt，或没有 OPENAI_API_KEY，或值为 null
    if (
      authObject.auth_mode === "chatgpt" ||
      !("OPENAI_API_KEY" in authObject) ||
      authObject.OPENAI_API_KEY === null
    ) {
      return "chatgpt_subscription"
    }
  }
  return "api_key"
}

export function hasCodexChatgptTokens(authJsonText: string): boolean {
  const { authObject } = parseCodexAuthJsonObject(authJsonText)
  if (!authObject) return false
  const tokens = authObject.tokens as Record<string, unknown> | undefined
  if (tokens && typeof tokens === "object") {
    return (
      typeof tokens.access_token === "string" && tokens.access_token.length > 0
    )
  }
  return false
}

export function extractCodexImportantValues(
  authJsonText: string,
  configTomlText: string
): CodexImportantValues {
  const parsedAuth = parseCodexAuthJsonObject(authJsonText)
  const authObject = parsedAuth.authObject ?? {}
  const toml = extractCodexTomlImportantValues(configTomlText)
  const hasExplicitProvider = Boolean(toml.modelProvider.trim())
  const activeProvider = hasExplicitProvider
    ? toml.modelProvider.trim()
    : CODEX_DEFAULT_MODEL_PROVIDER
  const providerBaseUrl = hasExplicitProvider
    ? (toml.providerBaseUrls[activeProvider] ?? "")
    : (toml.providerBaseUrls[CODEX_DEFAULT_MODEL_PROVIDER] ??
      toml.providerBaseUrls.openai ??
      "")
  const providerSupportsWebsockets =
    toml.providerSupportsWebsockets[activeProvider] ??
    (activeProvider === CODEX_DEFAULT_MODEL_PROVIDER
      ? toml.featureResponsesWebsocketsV2
      : false)
  return {
    apiBaseUrl: providerBaseUrl,
    apiKey:
      parsedAuth.error === null
        ? (pickFirstString(authObject, [
            "OPENAI_API_KEY",
            "OPENAI_API_TOKEN",
            "API_KEY",
          ]) ?? "")
        : null,
    model: toml.model,
    modelProvider: activeProvider,
    reasoningEffort: toml.modelReasoningEffort,
    providerOptions: buildCodexProviderOptions(
      activeProvider,
      toml.providerNames
    ),
    supportsWebsockets: providerSupportsWebsockets,
    skills: toml.featureSkills,
    serviceTierFast: toml.serviceTierFast,
  }
}

export function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

export function findTomlRootEndIndex(lines: string[]): number {
  for (let i = 0; i < lines.length; i += 1) {
    if (/^\[.*\]$/.test(lines[i].trim())) return i
  }
  return lines.length
}

export function findTomlRootAssignmentIndex(
  lines: string[],
  key: string
): number {
  const rootEnd = findTomlRootEndIndex(lines)
  for (let i = 0; i < rootEnd; i += 1) {
    const assignmentKey = parseTomlAssignmentKey(lines[i])
    if (assignmentKey === key) return i
  }
  return -1
}

export function preferredTomlRootInsertionIndex(
  lines: string[],
  key: string
): number {
  if (key === "model") {
    const providerIndex = findTomlRootAssignmentIndex(lines, "model_provider")
    return providerIndex >= 0 ? providerIndex : 0
  }
  if (key === "model_reasoning_effort") {
    const modelIndex = findTomlRootAssignmentIndex(lines, "model")
    return modelIndex >= 0 ? modelIndex + 1 : 0
  }
  let insertAt = findTomlRootEndIndex(lines)
  while (insertAt > 0 && lines[insertAt - 1].trim() === "") {
    insertAt -= 1
  }
  return insertAt
}

export function updateTomlRootStringKey(
  configTomlText: string,
  key: string,
  value: string
): string {
  const lineText = `${key} = ${JSON.stringify(value)}`
  const lines = configTomlText.split(/\r?\n/)
  const assignmentIndex = findTomlRootAssignmentIndex(lines, key)

  const nextValue = value.trim()
  if (!nextValue) {
    if (assignmentIndex >= 0) {
      lines.splice(assignmentIndex, 1)
    }
    return lines.join("\n").trim()
  }

  const insertAt = preferredTomlRootInsertionIndex(lines, key)
  if (assignmentIndex >= 0) {
    lines[assignmentIndex] = lineText
  } else {
    lines.splice(Math.max(0, insertAt), 0, lineText)
  }
  return lines.join("\n").trim()
}

export function updateTomlRootBooleanKey(
  configTomlText: string,
  key: string,
  value: boolean
): string {
  const lineText = `${key} = ${value ? "true" : "false"}`
  const lines = configTomlText.split(/\r?\n/)
  const assignmentIndex = findTomlRootAssignmentIndex(lines, key)
  if (assignmentIndex >= 0) {
    lines[assignmentIndex] = lineText
  } else {
    lines.splice(0, 0, lineText)
  }
  return lines.join("\n").trim()
}

export function findTomlSectionRange(
  lines: string[],
  sectionName: string
): { start: number; end: number } | null {
  const headerText = `[${sectionName}]`
  let sectionStart = -1
  let sectionEnd = lines.length
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim()
    if (sectionStart < 0) {
      if (trimmed === headerText) {
        sectionStart = i
      }
      continue
    }
    if (/^\[.*\]$/.test(trimmed)) {
      sectionEnd = i
      break
    }
  }
  if (sectionStart < 0) return null
  return { start: sectionStart, end: sectionEnd }
}

export function removeTomlSection(
  configTomlText: string,
  sectionName: string
): string {
  const lines = configTomlText.split(/\r?\n/)
  const range = findTomlSectionRange(lines, sectionName)
  if (!range) return configTomlText
  // Remove blank line before section header if present
  const removeStart =
    range.start > 0 && lines[range.start - 1].trim() === ""
      ? range.start - 1
      : range.start
  lines.splice(removeStart, range.end - removeStart)
  return lines.join("\n").trim()
}

export function upsertTomlSectionBooleanKey(
  configTomlText: string,
  sectionName: string,
  key: string,
  value: boolean | null
): string {
  const lines = configTomlText.split(/\r?\n/)
  const section = findTomlSectionRange(lines, sectionName)

  if (section) {
    let assignmentIndex = -1
    for (let i = section.start + 1; i < section.end; i += 1) {
      const assignmentKey = parseTomlAssignmentKey(lines[i])
      if (assignmentKey === key) {
        assignmentIndex = i
        break
      }
    }

    if (value === null) {
      if (assignmentIndex >= 0) {
        lines.splice(assignmentIndex, 1)
      }
      const refreshedSection = findTomlSectionRange(lines, sectionName)
      if (refreshedSection) {
        const hasEntries = lines
          .slice(refreshedSection.start + 1, refreshedSection.end)
          .some((rawLine) => {
            const line = rawLine.trim()
            return line !== "" && !line.startsWith("#")
          })
        if (!hasEntries) {
          const before = lines.slice(0, refreshedSection.start)
          const after = lines.slice(refreshedSection.end)
          while (before.length > 0 && before[before.length - 1].trim() === "") {
            before.pop()
          }
          while (after.length > 0 && after[0].trim() === "") {
            after.shift()
          }
          const merged =
            before.length > 0 && after.length > 0
              ? [...before, "", ...after]
              : [...before, ...after]
          return merged.join("\n").trim()
        }
      }
      return lines.join("\n").trim()
    }

    const lineText = `${key} = ${value ? "true" : "false"}`
    if (assignmentIndex >= 0) {
      lines[assignmentIndex] = lineText
    } else {
      let insertAt = section.end
      for (let i = section.end - 1; i > section.start; i -= 1) {
        if (lines[i].trim() !== "") {
          insertAt = i + 1
          break
        }
      }
      lines.splice(insertAt, 0, lineText)
    }
    return lines.join("\n").trim()
  }

  if (value === null) {
    return configTomlText.trim()
  }

  const lineText = `${key} = ${value ? "true" : "false"}`
  const insertAt = findTomlRootEndIndex(lines)
  const prefixBlank =
    insertAt > 0 && lines[insertAt - 1].trim() !== "" ? [""] : []
  const suffixBlank =
    insertAt < lines.length && lines[insertAt].trim() !== "" ? [""] : []
  lines.splice(
    insertAt,
    0,
    ...prefixBlank,
    `[${sectionName}]`,
    lineText,
    ...suffixBlank
  )
  return lines.join("\n").trim()
}

export function patchCodexProviderBaseUrl(
  configTomlText: string,
  provider: string,
  apiBaseUrl: string
): string {
  const trimmedProvider = provider.trim()
  if (!trimmedProvider) return configTomlText.trim()

  const nextApiBaseUrl = apiBaseUrl.trim()
  const lines = configTomlText.split(/\r?\n/)
  const sectionPattern = new RegExp(
    `^\\[\\s*model_providers\\.${escapeRegExp(trimmedProvider)}\\s*\\]$`
  )
  let sectionStart = -1
  let sectionEnd = lines.length
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim()
    if (sectionStart < 0) {
      if (sectionPattern.test(trimmed)) {
        sectionStart = i
      }
      continue
    }
    if (/^\[.*\]$/.test(trimmed)) {
      sectionEnd = i
      break
    }
  }

  if (sectionStart >= 0) {
    let baseUrlIndex = -1
    for (let i = sectionStart + 1; i < sectionEnd; i += 1) {
      const assignment = parseTomlStringAssignment(lines[i])
      if (!assignment || assignment.key !== "base_url") continue
      baseUrlIndex = i
      break
    }
    if (!nextApiBaseUrl) {
      if (baseUrlIndex >= 0) {
        lines.splice(baseUrlIndex, 1)
      }
      return lines.join("\n").trim()
    }

    const lineText = `base_url = ${JSON.stringify(nextApiBaseUrl)}`
    if (baseUrlIndex >= 0) {
      lines[baseUrlIndex] = lineText
    } else {
      lines.splice(sectionEnd, 0, lineText)
    }
    return lines.join("\n").trim()
  }

  if (!nextApiBaseUrl) return configTomlText.trim()

  const appended = configTomlText.trimEnd()
  const sectionText = `[model_providers.${trimmedProvider}]\nbase_url = ${JSON.stringify(nextApiBaseUrl)}`
  if (!appended) return sectionText
  return `${appended}\n\n${sectionText}`.trim()
}

export function patchCodexProviderField(
  configTomlText: string,
  provider: string,
  key: string,
  lineText: string
): string {
  const trimmedProvider = provider.trim()
  if (!trimmedProvider) return configTomlText.trim()

  const lines = configTomlText.split(/\r?\n/)
  const sectionPattern = new RegExp(
    `^\\[\\s*model_providers\\.${escapeRegExp(trimmedProvider)}\\s*\\]$`
  )
  let sectionStart = -1
  let sectionEnd = lines.length
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim()
    if (sectionStart < 0) {
      if (sectionPattern.test(trimmed)) {
        sectionStart = i
      }
      continue
    }
    if (/^\[.*\]$/.test(trimmed)) {
      sectionEnd = i
      break
    }
  }

  if (sectionStart >= 0) {
    let fieldIndex = -1
    for (let i = sectionStart + 1; i < sectionEnd; i += 1) {
      const assignmentKey = parseTomlAssignmentKey(lines[i])
      if (assignmentKey !== key) continue
      fieldIndex = i
      break
    }
    if (fieldIndex >= 0) {
      lines[fieldIndex] = lineText
    } else {
      let insertAt = sectionEnd
      while (insertAt > sectionStart + 1 && lines[insertAt - 1].trim() === "") {
        insertAt -= 1
      }
      lines.splice(insertAt, 0, lineText)
    }
    return lines.join("\n").trim()
  }

  const appended = configTomlText.trimEnd()
  const sectionText = `[model_providers.${trimmedProvider}]\n${lineText}`
  if (!appended) return sectionText
  return `${appended}\n\n${sectionText}`.trim()
}

/**
 * Result of reading `[model_providers.<provider>]` out of a config.toml draft.
 * "table absent" and "document unparsable" are deliberately distinct: an absent
 * table is a brand-new provider we should seed, a document we cannot parse is
 * one we must not touch.
 */
export type CodexProviderTableRead =
  | { status: "ok"; table: Record<string, unknown> | null }
  | { status: "unparsable" }

/**
 * Read-only view of one provider table. Every *write* in this file stays
 * text-based so user comments and key order survive; only the "is this field
 * already declared?" question goes through a real parser, because answering it
 * from text needs full TOML semantics — quoted keys containing dots, escape
 * decoding, dotted keys and inline tables — that a line scanner cannot supply.
 */
export function readCodexProviderTable(
  configTomlText: string,
  provider: string
): CodexProviderTableRead {
  const name = provider.trim()
  if (!name) return { status: "unparsable" }
  let parsed: unknown
  try {
    parsed = parseTomlDocument(configTomlText)
  } catch {
    return { status: "unparsable" }
  }
  if (!parsed || typeof parsed !== "object") return { status: "unparsable" }
  const providers = (parsed as Record<string, unknown>).model_providers
  if (!providers || typeof providers !== "object" || Array.isArray(providers)) {
    return { status: "ok", table: null }
  }
  const table = (providers as Record<string, unknown>)[name]
  if (!table || typeof table !== "object" || Array.isArray(table)) {
    return { status: "ok", table: null }
  }
  return { status: "ok", table: table as Record<string, unknown> }
}

/**
 * Mirrors the header half of codex's
 * `ModelProviderInfo::uses_openai_actor_authorization`. The ASCII-only fold
 * matches its `eq_ignore_ascii_case`.
 */
export function codexProviderUsesActorAuthorization(
  table: Record<string, unknown> | null
): boolean {
  const headers = table?.http_headers
  if (!headers || typeof headers !== "object" || Array.isArray(headers)) {
    return false
  }
  return Object.entries(headers as Record<string, unknown>).some(
    ([name, value]) =>
      name.replace(/[A-Z]/g, (char) => char.toLowerCase()) ===
        CODEX_ACTOR_AUTHORIZATION_HEADER &&
      typeof value === "string" &&
      value.trim() !== ""
  )
}

export function ensureCodexProviderDefaults(
  configTomlText: string,
  provider: string
): string {
  if (provider.trim() !== CODEX_DEFAULT_MODEL_PROVIDER) {
    return configTomlText
  }
  let next = configTomlText
  const current = extractCodexTomlImportantValues(next)
  const codegBaseUrl =
    current.providerBaseUrls[CODEX_DEFAULT_MODEL_PROVIDER] ?? ""
  next = patchCodexProviderField(
    next,
    CODEX_DEFAULT_MODEL_PROVIDER,
    "base_url",
    `base_url = ${JSON.stringify(codegBaseUrl)}`
  )
  next = patchCodexProviderField(
    next,
    CODEX_DEFAULT_MODEL_PROVIDER,
    "name",
    'name = "codeg"'
  )
  next = patchCodexProviderField(
    next,
    CODEX_DEFAULT_MODEL_PROVIDER,
    "wire_api",
    'wire_api = "responses"'
  )
  // `requires_openai_auth` is the one managed field a user legitimately owns:
  // codex defaults it to false, and its `uses_openai_actor_authorization()`
  // requires `!requires_openai_auth`, so forcing true silently disables the
  // actor-authorization path. Supply codeg's default only when the provider
  // does not already declare it — true is right for a provider *we* created
  // (key in auth.json, no env_key), never for one the user configured.
  // Read the original text: the three patches above never touch
  // `requires_openai_auth` or `http_headers`, and the original is what the
  // user actually authored.
  const read = readCodexProviderTable(
    configTomlText,
    CODEX_DEFAULT_MODEL_PROVIDER
  )
  const providerTable = read.status === "ok" ? read.table : null
  const alreadyDeclared =
    read.status !== "ok" ||
    (providerTable !== null &&
      Object.prototype.hasOwnProperty.call(
        providerTable,
        "requires_openai_auth"
      ))
  if (!alreadyDeclared && !codexProviderUsesActorAuthorization(providerTable)) {
    next = patchCodexProviderField(
      next,
      CODEX_DEFAULT_MODEL_PROVIDER,
      "requires_openai_auth",
      "requires_openai_auth = true"
    )
  }
  return next
}

export function patchCodexAuthJsonText(
  authJsonText: string,
  patch: { apiKey?: string; authMode?: "chatgpt" | null }
): {
  authJsonText: string
  recoveredFromInvalid: boolean
} {
  const parsed = parseCodexAuthJsonObject(authJsonText)
  const authObject =
    parsed.error === null && parsed.authObject ? { ...parsed.authObject } : {}
  if (typeof patch.apiKey === "string") {
    const apiKey = patch.apiKey.trim()
    if (apiKey) {
      authObject.OPENAI_API_KEY = apiKey
      delete authObject.API_KEY
    } else {
      delete authObject.OPENAI_API_KEY
      delete authObject.OPENAI_API_TOKEN
      delete authObject.API_KEY
    }
  }
  if ("authMode" in patch) {
    if (patch.authMode === "chatgpt") {
      authObject.auth_mode = "chatgpt"
      authObject.OPENAI_API_KEY = null
    } else {
      delete authObject.auth_mode
    }
  }
  return {
    authJsonText:
      Object.keys(authObject).length === 0
        ? ""
        : JSON.stringify(authObject, null, 2),
    recoveredFromInvalid: Boolean(parsed.error),
  }
}

export function patchCodexConfigTomlText(
  configTomlText: string,
  patch: {
    apiBaseUrl?: string
    model?: string
    modelProvider?: string
    modelReasoningEffort?: string
    supportsWebsockets?: boolean
    skills?: boolean
    serviceTierFast?: boolean
  }
): string {
  let nextTomlText = configTomlText
  if (typeof patch.modelProvider === "string") {
    const modelProvider = patch.modelProvider.trim()
    if (modelProvider) {
      nextTomlText = updateTomlRootStringKey(
        nextTomlText,
        "model_provider",
        modelProvider
      )
      nextTomlText = ensureCodexProviderDefaults(nextTomlText, modelProvider)
    }
  }
  if (typeof patch.model === "string") {
    nextTomlText = updateTomlRootStringKey(nextTomlText, "model", patch.model)
  }
  if (typeof patch.modelReasoningEffort === "string") {
    const reasoningEffort =
      normalizeCodexReasoningEffort(patch.modelReasoningEffort) ??
      CODEX_DEFAULT_REASONING_EFFORT
    nextTomlText = updateTomlRootStringKey(
      nextTomlText,
      "model_reasoning_effort",
      reasoningEffort
    )
  }
  if (typeof patch.apiBaseUrl === "string") {
    const tomlValues = extractCodexTomlImportantValues(nextTomlText)
    const modelProvider =
      patch.modelProvider?.trim() ||
      tomlValues.modelProvider.trim() ||
      CODEX_DEFAULT_MODEL_PROVIDER
    if (!tomlValues.modelProvider.trim() && patch.apiBaseUrl.trim()) {
      nextTomlText = updateTomlRootStringKey(
        nextTomlText,
        "model_provider",
        modelProvider
      )
    }
    nextTomlText = patchCodexProviderBaseUrl(
      nextTomlText,
      modelProvider,
      patch.apiBaseUrl
    )
    nextTomlText = ensureCodexProviderDefaults(nextTomlText, modelProvider)
  }
  if (typeof patch.supportsWebsockets === "boolean") {
    const tomlValues = extractCodexTomlImportantValues(nextTomlText)
    const modelProvider =
      patch.modelProvider?.trim() ||
      tomlValues.modelProvider.trim() ||
      CODEX_DEFAULT_MODEL_PROVIDER
    if (!tomlValues.modelProvider.trim()) {
      nextTomlText = updateTomlRootStringKey(
        nextTomlText,
        "model_provider",
        modelProvider
      )
    }
    nextTomlText = patchCodexProviderField(
      nextTomlText,
      modelProvider,
      "supports_websockets",
      `supports_websockets = ${patch.supportsWebsockets ? "true" : "false"}`
    )
    nextTomlText = ensureCodexProviderDefaults(nextTomlText, modelProvider)
  }
  const normalizedTomlValues = extractCodexTomlImportantValues(nextTomlText)
  if (normalizedTomlValues.model.trim()) {
    nextTomlText = updateTomlRootStringKey(
      nextTomlText,
      "model",
      normalizedTomlValues.model
    )
  }
  nextTomlText = updateTomlRootStringKey(
    nextTomlText,
    "model_reasoning_effort",
    normalizedTomlValues.modelReasoningEffort
  )
  const activeProvider =
    normalizedTomlValues.modelProvider.trim() || CODEX_DEFAULT_MODEL_PROVIDER
  const shouldEnableFeature = Boolean(
    normalizedTomlValues.providerSupportsWebsockets[activeProvider]
  )
  nextTomlText = upsertTomlSectionBooleanKey(
    nextTomlText,
    "features",
    "responses_websockets_v2",
    shouldEnableFeature ? true : null
  )
  if (typeof patch.skills === "boolean") {
    nextTomlText = upsertTomlSectionBooleanKey(
      nextTomlText,
      "features",
      "skills",
      patch.skills ? true : null
    )
  }
  if (typeof patch.serviceTierFast === "boolean") {
    nextTomlText = updateTomlRootStringKey(
      nextTomlText,
      "service_tier",
      patch.serviceTierFast ? "fast" : ""
    )
  }
  nextTomlText = updateTomlRootBooleanKey(
    nextTomlText,
    "disable_response_storage",
    true
  )
  const trimmed = nextTomlText.trim()
  return trimmed ? `${trimmed}\n` : ""
}

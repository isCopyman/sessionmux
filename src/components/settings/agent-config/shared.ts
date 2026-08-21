import { setCustomAgentDisplay } from "@/lib/custom-agents"
import { toErrorMessage } from "@/lib/app-error"
import type {
  AcpAgentInfo,
  AdapterInfo,
  AgentType,
  CheckStatus,
  CodexGranularApproval,
  CodexModelConfig,
  FixAction,
  PreflightResult,
} from "@/lib/types"
import type { ClaudeAuthMode, ClaudeEffortLevel } from "../claude-config-fields"
import type { ClaudeModelKey } from "./claude"
import type { GeminiAuthMode } from "./gemini"
import type {
  CodexApprovalPolicyChoice,
  CodexAuthMode,
  CodexReasoningEffort,
  CodexSandboxBaseline,
  CodexSandboxModeChoice,
} from "./codex"
import type { GrokAuthMethod } from "./grok"
import type { ClineProvider } from "./cline"

export interface AgentCheckState {
  result?: PreflightResult
  error?: string
}

export interface AgentDraft {
  enabled: boolean
  envText: string
  configText: string
  apiBaseUrl: string
  apiKey: string
  model: string
  claudeAuthMode: ClaudeAuthMode
  modelProviderId: number | null
  geminiAuthMode: GeminiAuthMode
  geminiApiKey: string
  googleApiKey: string
  googleCloudProject: string
  googleCloudLocation: string
  googleApplicationCredentials: string
  codexAuthMode: CodexAuthMode
  codexModelProvider: string
  codexProviderOptions: string[]
  codexReasoningEffort: CodexReasoningEffort
  codexSupportsWebsockets: boolean
  codexSkills: boolean
  codexServiceTierFast: boolean
  /** Sandbox / approval group — the thread defaults codex applies to turns it
   * starts itself (`/goal`, `/review`, `/compact`). Held as plain draft state
   * (not derived from `codexConfigTomlText`) and merged into config.toml
   * server-side on save. */
  codexApprovalPolicy: CodexApprovalPolicyChoice
  codexGranular: CodexGranularApproval
  codexSandboxMode: CodexSandboxModeChoice
  /** `writable_roots`, one absolute path per line. */
  codexWritableRootsText: string
  codexNetworkAccess: boolean
  codexExcludeTmpdirEnvVar: boolean
  codexExcludeSlashTmp: boolean
  /** The sandbox group as it was read off disk. A save sends only the fields
   * that differ from this, so neither the raw config.toml editor nor an
   * untouched control can revert the other. */
  codexSandboxBaseline: CodexSandboxBaseline
  /** Read-only diagnostics from the backend projection: `default_permissions`
   * makes codex ignore `sandbox_mode` entirely. */
  codexSandboxShadowed: boolean
  codexSandboxHasPermissionsTable: boolean
  claudeMainModel: string
  claudeReasoningModel: string
  claudeDefaultHaikuModel: string
  claudeDefaultSonnetModel: string
  claudeDefaultOpusModel: string
  claudeCustomModelOption: string
  claudeCustomModelOptionName: string
  claudeCustomModelOptionDescription: string
  claudeEffortLevel: ClaudeEffortLevel
  // Claude Code hardening toggles (native config `env`). `claudeSendAttributionHeader`
  // → CLAUDE_CODE_ATTRIBUTION_HEADER (on="1"/off="0"), default off (don't send).
  // `claudeDisableNonessentialTraffic` → CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC,
  // default on (disabled).
  claudeSendAttributionHeader: boolean
  claudeDisableNonessentialTraffic: boolean
  codexAuthJsonText: string
  codexConfigTomlText: string
  /** Structured codex custom-model list (mirrors the catalog source sidecar).
   *  Drives the model editor + `model_catalog_json` generation on save. */
  codexModelList: CodexModelConfig
  grokConfigTomlText: string
  // Grok authentication method (subscription via `grok login` vs XAI_API_KEY).
  // Recorded in env as GROK_AUTH_MODE; drives which credential body renders and
  // whether XAI_API_KEY is stripped from env on subscription.
  grokAuthMode: GrokAuthMethod
  // Grok structured controls (empty string = "unset / use default"). Backed by
  // ~/.grok/config.toml [ui].permission_mode / [models].default_reasoning_effort;
  // merged onto the current on-disk config server-side on save.
  grokPermissionMode: string
  grokReasoningEffort: string
  // Grok custom model (BYO endpoint) → [model.<id>] + [models].default. Numeric
  // fields are held as strings for their inputs and parsed on save.
  grokCustomModelId: string
  grokCustomBaseUrl: string
  grokCustomApiKey: string
  grokCustomApiBackend: string
  grokCustomContextWindow: string
  grokAutoCompactThreshold: string
  openCodeAuthJsonText: string
  openClawGatewayUrl: string
  openClawGatewayToken: string
  openClawSessionKey: string
  clineProvider: ClineProvider
  clineApiKey: string
  clineModel: string
  clineBaseUrl: string
  // Hermes — `apiKey`/`model`/`apiBaseUrl` are reused for the active provider's
  // key, model.default, and model.base_url. These carry the rest.
  hermesProvider: string
  hermesConfigYaml: string
  hermesHome: string
  hermesSetupCommand: string
  hermesModelCommand: string
}

export type RunningActionKind =
  | "download_binary"
  | "upgrade_binary"
  | "install_npx"
  | "upgrade_npx"
  | "uninstall_binary"
  | "uninstall_npx"
  | "redownload_binary"
  | "custom_install"
  | "install_uv"

export type UiFixAction =
  | FixAction
  | {
      label: string
      kind:
        | "download_binary"
        | "upgrade_binary"
        | "install_npx"
        | "upgrade_npx"
        | "uninstall_binary"
        | "uninstall_npx"
        | "install_opencode_plugins"
        | "custom_install"
      payload: string
      // When true, the fix renders as a greyed-out button (e.g. the uvx
      // agent-install action while the uv runtime isn't ready yet).
      disabled?: boolean
    }

export interface UiCheckItem {
  check_id: string
  label: string
  status: CheckStatus
  message: string
  fixes: UiFixAction[]
}

/**
 * Fix kinds that run a package operation. Only one of these may run at a time
 * across ALL agents, so while any of them is busy anywhere, every button whose
 * kind is listed here is disabled — and dimmed, so the lockout is visible on
 * agents other than the busy one.
 */
export const PACKAGE_ACTION_FIX_KINDS: ReadonlyArray<UiFixAction["kind"]> = [
  "download_binary",
  "upgrade_binary",
  "install_npx",
  "upgrade_npx",
  "uninstall_binary",
  "uninstall_npx",
  "redownload_binary",
  "install_opencode_plugins",
  "custom_install",
  "install_uv",
]

export type AcpTranslator = (
  key: string,
  values?: Record<string, string | number>
) => string

let acpTranslator: AcpTranslator | null = null

export function setAcpTranslator(next: AcpTranslator | null): void {
  acpTranslator = next
}

export function acpText(
  key: string,
  fallback: string,
  values?: Record<string, string | number>
): string {
  if (!acpTranslator) return fallback
  return acpTranslator(key, values)
}

/**
 * Publish a freshly fetched agent list into the custom-agent display map
 * (names + icons behind `getAgentLabel` / `getAgentIconUrl`). The map is
 * normally hydrated by `useAcpAgents`, but that hook lives in the workspace
 * surfaces — the settings window fetches its own list, so without this every
 * custom agent here falls back to the initial-letter glyph.
 */
export function publishAgentDisplay(list: AcpAgentInfo[]): void {
  setCustomAgentDisplay(
    list.map((agent) => ({
      agentType: agent.agent_type,
      name: agent.name,
      iconUrl: agent.icon_url,
    }))
  )
}

export function statusTone(status: CheckStatus): string {
  if (status === "pass") return "text-green-500"
  if (status === "warn") return "text-yellow-500"
  return "text-red-500"
}

export function summarizeChecks(
  checks: UiCheckItem[]
): CheckStatus | "unchecked" {
  if (checks.length === 0) return "unchecked"
  if (checks.some((check) => check.status === "fail")) return "fail"
  if (checks.some((check) => check.status === "warn")) return "warn"
  return "pass"
}

/**
 * Per-agent `env_json` knob deciding WHICH SIDE of the ACP connection reads
 * files and runs commands (`HostToolsPolicy`, Rust side). codeg advertises
 * `fs.readTextFile` / `terminal` by default, and an agent that sees them stops
 * using its own backends and delegates — so the work happens in CODEG's
 * process, outside any OS sandbox the agent applies to itself. Set to
 * {@link HOST_TOOLS_AGENT} and codeg advertises neither, so the agent does its
 * own I/O and its own sandbox covers it again (#436). Absent ⇒ codeg hosts.
 */
export const HOST_TOOLS_ENV = "CODEG_ACP_HOST_TOOLS"
export const HOST_TOOLS_AGENT = "agent"
export const HOST_TOOLS_DEFAULT = "default"

/**
 * Which real file each "native JSON config" editor writes — the agent CLI's OWN
 * config, never a codeg-private copy. Mirrors `agent_local_config_path` on the
 * Rust side; hard-coded here because the backend does not hand the resolved
 * path to the frontend, and an unmapped agent type just gets the path-less
 * wording. Naming the file is the whole point (users could not tell whether
 * codeg maintained its own settings).
 */
export const NATIVE_CONFIG_PATHS: Partial<Record<string, string>> = {
  claude_code: "~/.claude/settings.json",
  gemini: "~/.gemini/settings.json",
  open_code: "~/.config/opencode/opencode.json",
  cline: "~/.cline/data/globalState.json",
  kimi_code: "~/.kimi-code/config.toml",
}

export function envMapToText(env: Record<string, string>): string {
  return Object.entries(env)
    .map(([key, value]) => `${key}=${value}`)
    .join("\n")
}

export function parseEnvText(envText: string): Record<string, string> {
  const map: Record<string, string> = {}
  for (const rawLine of envText.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line || line.startsWith("#")) continue
    const idx = line.indexOf("=")
    if (idx <= 0) continue
    const key = line.slice(0, idx).trim()
    const value = line.slice(idx + 1).trim()
    if (!key) continue
    map[key] = value
  }
  return map
}

/**
 * Set (or, for an empty value, delete) exactly the given keys in a raw env
 * draft, leaving every other LINE byte-identical.
 *
 * Textual on purpose. The obvious implementation — parse to a map, patch,
 * serialize — rewrites the whole textarea, and the parser only understands
 * `KEY=VALUE`: a comment, a blank line, and a half-typed `NEW_PROXY` all
 * vanish. These patches run on refresh and on save completion, so that would
 * silently delete what the user is still typing in the raw editor next to the
 * structured panel that triggered the save.
 *
 * A key appearing on several lines collapses to one (its patched value), which
 * matches how `parseEnvText` reads the draft afterwards.
 */
export function patchEnvText(
  envText: string,
  patch: Record<string, string | undefined>
): string {
  // `key in patch` would also answer yes for `constructor`, `toString` and the
  // rest of Object.prototype — all of them legal env var names — and then read
  // a function where a string was expected. Own properties only.
  const owns = (key: string) => Object.prototype.hasOwnProperty.call(patch, key)
  const pending = new Set(
    Object.keys(patch).filter((key) => (patch[key]?.trim() ?? "") !== "")
  )
  const lines = envText === "" ? [] : envText.split(/\r?\n/)
  const kept: string[] = []
  for (const rawLine of lines) {
    const line = rawLine.trim()
    const idx = line.startsWith("#") ? -1 : line.indexOf("=")
    const key = idx > 0 ? line.slice(0, idx).trim() : ""
    if (!key || !owns(key)) {
      kept.push(rawLine)
      continue
    }
    const value = patch[key]?.trim() ?? ""
    // Empty ⇒ the key is being removed; a duplicate line for a key already
    // emitted goes too, so the result reads back as the value just written.
    if (!value || !pending.delete(key)) continue
    kept.push(`${key}=${value}`)
  }
  if (pending.size > 0) {
    // A key with no line yet goes after the last real one, not after the blank
    // line the user may be about to type into.
    let end = kept.length
    while (end > 0 && kept[end - 1].trim() === "") end -= 1
    const tail = kept.splice(end)
    for (const key of pending) kept.push(`${key}=${patch[key]?.trim() ?? ""}`)
    kept.push(...tail)
  }
  return kept.join("\n")
}

/**
 * Whether this agent's env draft hands the ACP fs/terminal channels back to the
 * agent — see {@link HOST_TOOLS_ENV}. Anything other than the exact sentinel
 * (including a hand-typed `default`) reads as off, matching the Rust resolver,
 * which fails OPEN on an unrecognized value rather than silently withholding.
 *
 * Reads the per-agent layer ONLY. When the key is absent and an operator has
 * exported `CODEG_ACP_HOST_TOOLS=agent` in codeg's own environment, the switch
 * renders off while the next connection actually withholds the channels — the
 * display understates how restricted the agent is. Showing that inherited state
 * would need the backend to report its resolved process-env value; until then
 * the error is in the safe direction, and {@link setHostToolsAgentMode} makes
 * the per-agent value authoritative the moment the user touches the switch.
 */
export function hostToolsAgentModeEnabled(envText: string): boolean {
  return parseEnvText(envText)[HOST_TOOLS_ENV] === HOST_TOOLS_AGENT
}

/**
 * Flip the knob in an env draft, always writing an EXPLICIT value — including
 * `default` for off, rather than deleting the key.
 *
 * Deleting would be tidier but wrong: the backend resolves this knob as
 * `env_json` first, then codeg's own process env. An operator who exported
 * `CODEG_ACP_HOST_TOOLS=agent` process-wide makes "absent" mean `agent`, so a
 * toggle that cleared the key on OFF could not turn the mode off at all — the
 * switch would read false while the next connection still withheld the
 * channels. Writing the value the user actually chose makes the per-agent
 * setting authoritative in both directions.
 */
export function setHostToolsAgentMode(
  envText: string,
  enabled: boolean
): string {
  return patchEnvText(envText, {
    [HOST_TOOLS_ENV]: enabled ? HOST_TOOLS_AGENT : HOST_TOOLS_DEFAULT,
  })
}

export interface ImportantEnvKeys {
  apiBaseUrl: string[]
  apiKey: string[]
  model: string[]
}

export const GEMINI_ENV_KEYS = {
  baseUrl: "GOOGLE_GEMINI_BASE_URL",
  legacyBaseUrl: "GEMINI_BASE_URL",
  geminiApiKey: "GEMINI_API_KEY",
  legacyGeminiApiKey: "GOOGLE_GEMINI_API_KEY",
  googleApiKey: "GOOGLE_API_KEY",
  cloudProject: "GOOGLE_CLOUD_PROJECT",
  cloudProjectLegacy: "GOOGLE_CLOUD_PROJECT_ID",
  cloudLocation: "GOOGLE_CLOUD_LOCATION",
  applicationCredentials: "GOOGLE_APPLICATION_CREDENTIALS",
  model: "GEMINI_MODEL",
} as const

export type ImportantConfigKey =
  | "apiBaseUrl"
  | "apiKey"
  | "model"
  | ClaudeModelKey
export type ImportantDraftPatch = Partial<Pick<AgentDraft, ImportantConfigKey>>

export interface ConfigParseResult {
  config: Record<string, unknown>
  error: string | null
}

export function importantEnvKeysByAgent(
  agentType: AgentType
): ImportantEnvKeys {
  if (agentType === "claude_code") {
    return {
      apiBaseUrl: ["ANTHROPIC_BASE_URL", "OPENAI_BASE_URL", "API_BASE_URL"],
      apiKey: ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"],
      model: ["ANTHROPIC_MODEL", "OPENAI_MODEL", "MODEL"],
    }
  }
  if (agentType === "gemini") {
    return {
      apiBaseUrl: ["GOOGLE_GEMINI_BASE_URL", "GEMINI_BASE_URL", "API_BASE_URL"],
      apiKey: [
        GEMINI_ENV_KEYS.geminiApiKey,
        GEMINI_ENV_KEYS.googleApiKey,
        GEMINI_ENV_KEYS.legacyGeminiApiKey,
        "API_KEY",
      ],
      model: ["GEMINI_MODEL", "MODEL"],
    }
  }
  if (agentType === "grok") {
    // Grok's non-interactive credential is XAI_API_KEY (mirrors the backend
    // `agent_env_keys(Grok)`). Model/endpoint have working env overrides too:
    // GROK_DEFAULT_MODEL and GROK_XAI_API_BASE_URL (both read by the Grok binary;
    // XAI_API_BASE_URL is also accepted). XAI_MODEL is NOT read by Grok.
    return {
      apiBaseUrl: ["GROK_XAI_API_BASE_URL", "XAI_API_BASE_URL", "API_BASE_URL"],
      // Only XAI_API_KEY is a real Grok credential; the generic API_KEY alias is
      // NOT read by Grok, so including it would let the auth panel report
      // "configured" for a key the agent never uses.
      apiKey: ["XAI_API_KEY"],
      model: ["GROK_DEFAULT_MODEL", "MODEL"],
    }
  }
  if (agentType === "deepseek") {
    // The endpoint knob is DEEPSEEK_BASE_URL (read per request by the
    // `llm-deepseek` adapter through the launch-environment snapshot, which
    // falls back to `process.env`). DEEPSEEK_ACP_PROVIDER is NOT it — that's
    // the provider ROUTE id, so binding a model provider to it would write a
    // URL into a registry key. Mirrors the backend `agent_env_keys(DeepSeek)`;
    // generic OPENAI_*/API_KEY aliases are NOT read.
    return {
      apiBaseUrl: ["DEEPSEEK_BASE_URL"],
      apiKey: ["DEEPSEEK_API_KEY"],
      model: ["DEEPSEEK_ACP_MODEL"],
    }
  }
  if (agentType === "qoder") {
    // Qoder's non-interactive credential is QODER_PERSONAL_ACCESS_TOKEN — the
    // only way to authenticate a headless/server/Docker install, where the
    // `qoder login` browser flow cannot run (an interactive login still
    // outranks it). QODER_MODEL is the env twin of `-m/--model`. There is no
    // real endpoint override, so QODER_BASE_URL is an inert placeholder —
    // mirrors the backend `agent_env_keys(Qoder)` — kept as a single-element
    // list rather than empty: `patchEnvByImportantKey` below indexes
    // `keys.apiBaseUrl[0]` unguarded, and an empty list would patch an env var
    // literally named "undefined". Generic OPENAI_*/API_KEY aliases are NOT
    // read by Qoder.
    return {
      apiBaseUrl: ["QODER_BASE_URL"],
      apiKey: ["QODER_PERSONAL_ACCESS_TOKEN"],
      model: ["QODER_MODEL"],
    }
  }
  return {
    apiBaseUrl: ["OPENAI_BASE_URL", "API_BASE_URL"],
    apiKey: ["OPENAI_API_KEY", "API_KEY"],
    model: ["OPENAI_MODEL", "MODEL"],
  }
}

export function parseConfigJsonText(configText: string): ConfigParseResult {
  const trimmed = configText.trim()
  if (!trimmed) return { config: {}, error: null }

  try {
    const parsed = JSON.parse(trimmed) as unknown
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {
        config: {},
        error: acpText(
          "errors.nativeJsonMustBeObject",
          "Native JSON config must be an object"
        ),
      }
    }
    return { config: parsed as Record<string, unknown>, error: null }
  } catch (err) {
    const message = toErrorMessage(err)
    return {
      config: {},
      error: acpText(
        "errors.nativeJsonInvalid",
        "Native JSON config format error: {message}",
        { message }
      ),
    }
  }
}

export function asObjectRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

export function envFromConfig(
  config: Record<string, unknown>
): Record<string, string> {
  const raw = config.env
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return {}
  }

  const map: Record<string, string> = {}
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    if (typeof value !== "string") continue
    const trimmedKey = key.trim()
    const trimmedValue = value.trim()
    if (!trimmedKey || !trimmedValue) continue
    map[trimmedKey] = trimmedValue
  }
  return map
}

export function pickFirstString(
  source: Record<string, unknown>,
  keys: string[]
): string | null {
  for (const key of keys) {
    const value = source[key]
    if (typeof value !== "string") continue
    const trimmed = value.trim()
    if (trimmed) return trimmed
  }
  return null
}

export function findEnvValue(
  env: Record<string, string>,
  keys: string[]
): string {
  for (const key of keys) {
    const value = env[key]
    if (!value) continue
    const trimmed = value.trim()
    if (trimmed) return trimmed
  }
  return ""
}

/**
 * Compare original and current config objects. For any key present in
 * original but missing in current, set it to `null` in the result so
 * the backend merge can delete it from the file on disk.
 */
export function markRemovedKeysNull(
  original: Record<string, unknown>,
  current: Record<string, unknown>
): Record<string, unknown> {
  const result: Record<string, unknown> = { ...current }
  for (const key of Object.keys(original)) {
    if (!(key in result)) {
      result[key] = null
    } else if (
      original[key] &&
      typeof original[key] === "object" &&
      !Array.isArray(original[key]) &&
      result[key] &&
      typeof result[key] === "object" &&
      !Array.isArray(result[key])
    ) {
      result[key] = markRemovedKeysNull(
        original[key] as Record<string, unknown>,
        result[key] as Record<string, unknown>
      )
    }
  }
  return result
}

/**
 * Build the `config_json` payload for a merge-strategy agent save (Gemini /
 * OpenClaw). Diffs the current config against the original so removed keys
 * become explicit `null`s the backend merge deletes from disk — crucially even
 * when the current config emptied to "" (e.g. the last env flag toggled off),
 * which would otherwise serialize to a null `config_json` no-op and leave the
 * stale key on disk. Returns `null` when both sides are empty (nothing to
 * write, no empty file created). Pure — shared by `persistConfig` and tests.
 * Claude Code does not use this path: follow-default must not write
 * `~/.claude/settings.json`.
 */
export function buildMergeConfigPayload(
  currentConfigText: string,
  originalConfigText: string | null | undefined
): string | null {
  const currentConfig = parseConfigJsonText(currentConfigText).config
  const originalConfig = originalConfigText
    ? parseConfigJsonText(originalConfigText).config
    : {}
  if (
    Object.keys(currentConfig).length === 0 &&
    Object.keys(originalConfig).length === 0
  ) {
    return null
  }
  return JSON.stringify(
    markRemovedKeysNull(originalConfig, currentConfig),
    null,
    2
  )
}

export function normalizeConfigText(configText: string): string {
  const parseResult = parseConfigJsonText(configText)
  if (parseResult.error) return configText.trim()
  if (Object.keys(parseResult.config).length === 0) return ""
  return JSON.stringify(parseResult.config, null, 2)
}

export function compareVersion(a: string, b: string): number {
  const toParts = (value: string): number[] => {
    const normalized = value.trim().replace(/^[^\d]*/, "")
    return normalized.split(".").map((part) => Number.parseInt(part, 10) || 0)
  }
  const left = toParts(a)
  const right = toParts(b)
  const len = Math.max(left.length, right.length)
  for (let i = 0; i < len; i += 1) {
    const lv = left[i] ?? 0
    const rv = right[i] ?? 0
    if (lv !== rv) return lv > rv ? 1 : -1
  }
  return 0
}

export function hasComparableVersion(
  value: string | null | undefined
): value is string {
  return Boolean(value && /\d/.test(value) && value.includes("."))
}

// Mirror of the backend `sanitize_custom_version`: a custom install version
// tolerates a leading `v`, must start with a digit, must be dotted (e.g.
// `1.2.3`), and may only contain `[0-9A-Za-z.-+]` (semver pre-release/build +
// calendar versions). Rejects npm dist-tags like `latest`, bare majors like
// `2`, and anything with spaces / `@`.
export function isValidCustomVersion(value: string): boolean {
  const normalized = value.trim().replace(/^[vV]/, "")
  return /^[0-9][0-9A-Za-z.\-+]*$/.test(normalized) && normalized.includes(".")
}

/**
 * The explainer card for agents whose codeg entry is a third-party ACP
 * *adapter* rather than the vendor's own CLI — Claude Code and Codex.
 *
 * Ten of the twelve built-ins install the vendor CLI itself, so a user's
 * existing global install is simply detected. These two are the exception:
 * neither `claude` nor `codex` speaks ACP, so codeg installs `claude-agent-acp`
 * / `codex-acp` instead, and the launch gate looks for THAT command. Without
 * this card the user only sees "Not installed" next to an agent they demonstrably
 * have — by far the most-reported confusion.
 *
 * Returns `null` for every non-adapter agent (backend decides, via
 * `PreflightResult.adapter`), and while preflight hasn't resolved yet.
 *
 * Deliberately carries NO install action: the Version Status card directly below
 * already has one, and two install buttons on adjacent cards only breeds doubt
 * about which is the right one.
 */
export function buildAcpAdapterCheck(
  adapter: AdapterInfo | null | undefined
): UiCheckItem | null {
  if (!adapter) return null

  const values = {
    nativeLabel: adapter.native_label,
    nativeCmd: adapter.native_cmd,
    nativePath: adapter.native_path ?? "",
    adapterPackage: adapter.adapter_package,
    adapterCmd: adapter.adapter_cmd,
    configDir: adapter.shared_config_dir,
  }

  // Installed → `pass`, so renderCheck collapses it: the relationship stays
  // documented for anyone who wonders later, without nagging a working setup.
  const installed = adapter.adapter_installed
  const sawNative = Boolean(adapter.native_path)
  // The English fallbacks mirror the four i18n messages one-for-one (they are
  // what renders if no translator is mounted), so each state keeps the detail
  // that state is about — above all, the path we found the vendor CLI at.
  const split = `Codeg drives agents over ACP and the ${adapter.native_label} does not speak ACP, so Codeg needs a separate adapter package, ${adapter.adapter_package}.`
  const coexist = `It ships its own runtime, never modifies or replaces your ${adapter.native_cmd} command, and reads the same ${adapter.shared_config_dir} — your existing sign-in and settings carry over.`
  const [key, fallback] = installed
    ? sawNative
      ? [
          "adapter.readyWithNative",
          `Adapter ${adapter.adapter_cmd} is installed — that is what Codeg launches, not your own ${adapter.native_cmd} at ${adapter.native_path}. They are separate packages that coexist, and both read ${adapter.shared_config_dir}.`,
        ]
      : [
          "adapter.ready",
          `Adapter ${adapter.adapter_cmd} is installed — that is what Codeg launches. It ships its own runtime, so the ${adapter.native_label} is not required.`,
        ]
    : sawNative
      ? [
          "adapter.missingWithNative",
          `Found your own ${adapter.native_label} at ${adapter.native_path}. ${split} ${coexist} Install it below.`,
        ]
      : [
          "adapter.missing",
          `${split} It ships its own runtime, so the ${adapter.native_cmd} CLI is not required first; if you do have it, the two coexist and share the same ${adapter.shared_config_dir} sign-in and settings. Install it below.`,
        ]

  return {
    check_id: "acp_adapter",
    label: acpText("adapter.label", "ACP adapter"),
    status: installed ? "pass" : "warn",
    message: acpText(key, fallback, values),
    fixes: [
      {
        label: acpText("adapter.learnMore", "Learn more"),
        kind: "open_url",
        payload: adapter.docs_url,
      },
    ],
  }
}

// `uvReady` reports whether the uv runtime (uvx) is installed — only meaningful
// for uvx agents (custom Python-package agents; built-in Hermes moved to the
// npm bridge). Derived from the uv preflight check by the caller. uvx agents
// need uv installed before their package can be prepared, so when uv isn't
// ready every managed install/upgrade action is surfaced disabled and the
// user is pointed at the separate "Install uv" preflight action.
export function buildVersionCheck(
  agent: AcpAgentInfo,
  uvReady: boolean = true
): UiCheckItem | null {
  if (
    agent.distribution_type !== "binary" &&
    agent.distribution_type !== "npx" &&
    agent.distribution_type !== "uvx"
  )
    return null

  const remoteVersion = agent.registry_version ?? "unknown"
  const localVersion =
    agent.installed_version ?? acpText("version.notInstalled", "Not installed")
  // A manually written definition has no registry behind it — its stored
  // version is whatever the user typed — so "Remote:" would be comparing
  // against noise. Every message shows the local side alone.
  const manualSource = agent.custom_source === "manual"
  const versionText = manualSource
    ? acpText("version.localOnly", "Local: {localVersion}", { localVersion })
    : acpText(
        "version.remoteLocal",
        "Remote: {remoteVersion} · Local: {localVersion}",
        { remoteVersion, localVersion }
      )
  const installAction: RunningActionKind =
    agent.distribution_type === "binary" ? "download_binary" : "install_npx"
  const upgradeAction: RunningActionKind =
    agent.distribution_type === "binary" ? "upgrade_binary" : "upgrade_npx"
  const uninstallAction: RunningActionKind =
    agent.distribution_type === "binary" ? "uninstall_binary" : "uninstall_npx"

  // uvx agents need the uv runtime before any managed install/upgrade can
  // run. Surface a single blocked state pointing at the separate "Install
  // uv" preflight action below, with the agent-install action shown disabled.
  // This covers both the fresh case (available=false) and the rare system-CLI
  // case (available=true via the agent's own PATH CLI, but uvx still missing).
  // Uninstall stays available even without uv — it only clears the prepared
  // marker — so a prepared package can still be removed when uv is gone.
  if (agent.distribution_type === "uvx" && !uvReady) {
    const blockedFixes: UiFixAction[] = [
      {
        label: acpText("actions.install", "Install"),
        kind: installAction,
        payload: agent.agent_type,
        disabled: true,
      },
    ]
    if (agent.installed_version) {
      blockedFixes.push({
        label: acpText("actions.uninstall", "Uninstall"),
        kind: uninstallAction,
        payload: agent.agent_type,
      })
    }
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "warn",
      message: acpText(
        "version.uvxNotReady",
        "{versionText}. The uv runtime isn't installed — install it from the uv check below to use this agent.",
        { versionText }
      ),
      fixes: blockedFixes,
    }
  }

  // Only binary agents can be genuinely platform-unsupported (no binary for
  // this platform). uvx runs everywhere — a uvx agent that reaches here (uv
  // treated as ready, i.e. preflight unknown) falls through to an actionable
  // install rather than a dead-end "unsupported" message.
  if (!agent.available && agent.distribution_type !== "uvx") {
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "fail",
      message: acpText(
        "version.platformUnsupported",
        "{versionText}. Current platform does not support this agent.",
        { versionText }
      ),
      fixes: [],
    }
  }

  // Custom-version install is offered in every installable state (and stays
  // available after a version is installed, so users can switch versions).
  // Binary agents need the registry version present to template the download URL.
  // uvx agents pin their version in the package spec, so custom-version
  // install does not apply (the backend ignores the override).
  const supportsCustomInstall =
    agent.distribution_type === "npx" ||
    (agent.distribution_type === "binary" && Boolean(agent.registry_version))
  const customInstallFix: UiFixAction = {
    label: acpText("actions.customInstall", "Custom install"),
    kind: "custom_install",
    payload: agent.agent_type,
  }
  const withCustomInstall = (fixes: UiFixAction[]): UiFixAction[] =>
    supportsCustomInstall ? [...fixes, customInstallFix] : fixes

  if (!agent.installed_version) {
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "fail",
      message: acpText(
        "version.clickInstall",
        "{versionText}. Click Install on the right.",
        { versionText }
      ),
      fixes: withCustomInstall([
        {
          label: acpText("actions.install", "Install"),
          kind: installAction,
          payload: agent.agent_type,
        },
      ]),
    }
  }

  // Manual definitions stop here: installed is the whole story, and the
  // registry-comparison branches below would only manufacture "upgrade
  // available" noise against a user-typed version.
  if (manualSource) {
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "pass",
      message: acpText("version.localInstalled", "{versionText}. Installed.", {
        versionText,
      }),
      fixes: withCustomInstall([
        {
          label: acpText("actions.uninstall", "Uninstall"),
          kind: uninstallAction,
          payload: agent.agent_type,
        },
      ]),
    }
  }

  if (
    agent.registry_version &&
    hasComparableVersion(agent.registry_version) &&
    !hasComparableVersion(agent.installed_version)
  ) {
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "warn",
      message: acpText(
        "version.localUnrecognized",
        "{versionText}. Local version is not comparable; try upgrade to overwrite install.",
        { versionText }
      ),
      fixes: withCustomInstall([
        {
          label: acpText("actions.upgrade", "Upgrade"),
          kind: upgradeAction,
          payload: agent.agent_type,
        },
        {
          label: acpText("actions.uninstall", "Uninstall"),
          kind: uninstallAction,
          payload: agent.agent_type,
        },
      ]),
    }
  }

  if (
    hasComparableVersion(agent.registry_version) &&
    hasComparableVersion(agent.installed_version) &&
    compareVersion(agent.installed_version, agent.registry_version) < 0
  ) {
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "warn",
      message: acpText(
        "version.upgradeAvailable",
        "{versionText}. Upgrade available.",
        { versionText }
      ),
      fixes: withCustomInstall([
        {
          label: acpText("actions.upgrade", "Upgrade"),
          kind: upgradeAction,
          payload: agent.agent_type,
        },
        {
          label: acpText("actions.uninstall", "Uninstall"),
          kind: uninstallAction,
          payload: agent.agent_type,
        },
      ]),
    }
  }

  if (!agent.registry_version) {
    return {
      check_id: "version_status",
      label: acpText("version.statusLabel", "Version Status"),
      status: "warn",
      message: acpText(
        "version.remoteUnavailable",
        "{versionText}. Remote version is currently unavailable.",
        { versionText }
      ),
      fixes: withCustomInstall([
        {
          label: acpText("actions.uninstall", "Uninstall"),
          kind: uninstallAction,
          payload: agent.agent_type,
        },
      ]),
    }
  }

  return {
    check_id: "version_status",
    label: acpText("version.statusLabel", "Version Status"),
    status: "pass",
    message: acpText("version.latest", "{versionText}. Already latest.", {
      versionText,
    }),
    fixes: withCustomInstall([
      {
        label: acpText("actions.uninstall", "Uninstall"),
        kind: uninstallAction,
        payload: agent.agent_type,
      },
    ]),
  }
}

export function getAgentChecks(
  agent: AcpAgentInfo,
  current?: AgentCheckState
): UiCheckItem[] {
  // For uvx agents, only treat uv as not-ready when the preflight result is
  // present AND its uv check isn't passing. With no result yet (or an errored
  // preflight) stay optimistic — otherwise we'd block the version-status
  // install while the "Install uv" button (which lives in that same preflight
  // result) is absent, a dead end. When the result IS present, the button is
  // present alongside it, so blocking is always paired with an actionable fix.
  const uvCheck = current?.result?.checks?.find(
    (check) => check.check_id === "uv_available"
  )
  const uvReady =
    agent.distribution_type !== "uvx" || !uvCheck || uvCheck.status === "pass"
  const versionCheck = buildVersionCheck(agent, uvReady)
  const remoteChecks: UiCheckItem[] = (current?.result?.checks ?? []).map(
    (check) => ({
      ...check,
      fixes: [...check.fixes],
    })
  )
  // The adapter explainer goes FIRST: it answers "why does this say not
  // installed when I have the CLI?" before the Version Status card below it
  // offers the Install that fixes it.
  const adapterCheck = buildAcpAdapterCheck(current?.result?.adapter)
  return [adapterCheck, versionCheck, ...remoteChecks].filter(
    (check): check is UiCheckItem => check != null
  )
}

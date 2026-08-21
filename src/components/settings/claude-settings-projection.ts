/**
 * Project a profile's `settings.json` onto the flat shape {@link ClaudeConfigValue}
 * that {@link ClaudeConfigFields} edits, and back again.
 *
 * A profile has two stores and they are not rivals: three dedicated columns
 * (`baseUrl` / `authToken` / `model`) that the backend merges *into* the
 * materialized file, and the raw `settings.json` for everything else. See
 * `materialize_managed_profile` in `src-tauri/src/commands/claude_profile.rs`:
 * the raw object is the base, `record.env` overlays it, and the dedicated
 * fields win last. This module mirrors that order so what the form shows is
 * what the child process will get — the alternative, a second typed field per
 * settings key, is the "two editors for one setting" that made the old panel
 * unreadable.
 *
 * Nothing here adds storage. Every field that has no column is read from and
 * written to `settings.json`, which is why adding one needs no schema change.
 */

import {
  EMPTY_CLAUDE_CONFIG_VALUE,
  type ClaudeConfigValue,
  type ClaudeEffortLevel,
} from "./claude-config-fields"

/** Where each column-less field lives inside `settings.json`'s `env` block. */
const ENV_KEYS = {
  reasoningModel: "ANTHROPIC_REASONING_MODEL",
  haikuModel: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  sonnetModel: "ANTHROPIC_DEFAULT_SONNET_MODEL",
  opusModel: "ANTHROPIC_DEFAULT_OPUS_MODEL",
  customModelOption: "ANTHROPIC_CUSTOM_MODEL_OPTION",
  customModelOptionName: "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
  customModelOptionDescription: "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
} as const

const ATTRIBUTION_HEADER_ENV_KEY = "CLAUDE_CODE_ATTRIBUTION_HEADER"
const NONESSENTIAL_TRAFFIC_ENV_KEY = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"
const EFFORT_LEVEL_KEY = "effortLevel"

/** Connection keys an "official subscription" profile must not carry. */
const CONNECTION_ENV_KEYS = [
  "ANTHROPIC_BASE_URL",
  "ANTHROPIC_AUTH_TOKEN",
  "ANTHROPIC_API_KEY",
] as const

const EFFORT_LEVELS: readonly ClaudeEffortLevel[] = [
  "",
  "low",
  "medium",
  "high",
  "xhigh",
]

/** The three fields a profile stores as columns rather than in the file. */
export interface ClaudeProfileColumns {
  baseUrl: string
  authToken: string
  model: string
}

type JsonObject = Record<string, unknown>

function parseObject(text: string): JsonObject | null {
  const trimmed = text.trim()
  if (!trimmed) return {}
  let parsed: unknown
  try {
    parsed = JSON.parse(trimmed)
  } catch {
    return null
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return null
  }
  return parsed as JsonObject
}

function envBlock(root: JsonObject): Record<string, unknown> {
  const env = root.env
  if (typeof env === "object" && env !== null && !Array.isArray(env)) {
    return env as Record<string, unknown>
  }
  return {}
}

function readString(bag: Record<string, unknown>, key: string): string {
  const value = bag[key]
  return typeof value === "string" ? value : ""
}

/**
 * `"1"` is on and `"0"` is off, matching how the panel has always written
 * these. Anything else (unset, or a value someone typed by hand) falls back to
 * the documented default rather than guessing.
 */
function readFlag(
  bag: Record<string, unknown>,
  key: string,
  fallback: boolean
): boolean {
  const value = bag[key]
  if (value === "1") return true
  if (value === "0") return false
  return fallback
}

function normalizeEffort(value: unknown): ClaudeEffortLevel {
  if (typeof value !== "string") return ""
  // claude-agent-acp >= 0.37 writes the sentinel "default"; collapse it so the
  // picker's own "default" placeholder stays canonical either way.
  const normalized = value.trim().toLowerCase()
  if (normalized === "default") return ""
  return EFFORT_LEVELS.includes(normalized as ClaudeEffortLevel)
    ? (normalized as ClaudeEffortLevel)
    : ""
}

/**
 * Read the form's value out of a profile. Unparseable text yields the defaults
 * for the file-backed fields; the columns are still honoured, so a broken
 * `settings.json` does not blank out the endpoint the user can see.
 */
export function readClaudeConfig(
  settingsJson: string,
  columns: ClaudeProfileColumns
): ClaudeConfigValue {
  const root = parseObject(settingsJson) ?? {}
  const env = envBlock(root)
  const baseUrl = columns.baseUrl || readString(env, "ANTHROPIC_BASE_URL")
  // Both spellings are credentials. Reading only AUTH_TOKEN made a file that
  // authenticates with ANTHROPIC_API_KEY read back as a subscription profile —
  // the form then hid the connection fields on a profile that was still
  // sending a key. AUTH_TOKEN wins when a file carries both, matching the
  // write path, which only ever emits that one.
  const authToken =
    columns.authToken ||
    readString(env, "ANTHROPIC_AUTH_TOKEN") ||
    readString(env, "ANTHROPIC_API_KEY")
  return {
    ...EMPTY_CLAUDE_CONFIG_VALUE,
    // A profile has no provider binding, so there are only two honest answers
    // here: it points somewhere, or it rides the CLI's own login. A stored
    // token shows up as its mask in `columns.authToken`, so this reads the
    // credential's presence directly rather than being told about it.
    authMode: baseUrl || authToken ? "custom" : "official_subscription",
    apiBaseUrl: baseUrl,
    apiKey: authToken,
    mainModel: columns.model || readString(env, "ANTHROPIC_MODEL"),
    reasoningModel: readString(env, ENV_KEYS.reasoningModel),
    haikuModel: readString(env, ENV_KEYS.haikuModel),
    sonnetModel: readString(env, ENV_KEYS.sonnetModel),
    opusModel: readString(env, ENV_KEYS.opusModel),
    customModelOption: readString(env, ENV_KEYS.customModelOption),
    customModelOptionName: readString(env, ENV_KEYS.customModelOptionName),
    customModelOptionDescription: readString(
      env,
      ENV_KEYS.customModelOptionDescription
    ),
    effortLevel: normalizeEffort(root[EFFORT_LEVEL_KEY]),
    sendAttributionHeader: readFlag(env, ATTRIBUTION_HEADER_ENV_KEY, false),
    disableNonessentialTraffic: readFlag(
      env,
      NONESSENTIAL_TRAFFIC_ENV_KEY,
      true
    ),
  }
}

export interface ClaudeConfigWriteResult {
  settingsJson: string
  columns: Partial<ClaudeProfileColumns>
}

function setEnv(env: Record<string, unknown>, key: string, value: string) {
  // An empty field means "not set". Writing "" would survive into the
  // materialized file as a key the CLI then exports as an empty variable.
  if (value.trim()) env[key] = value
  else delete env[key]
}

/**
 * Apply one patch from the form.
 *
 * Returns `null` when the existing text is not a JSON object: the typed fields
 * cannot rewrite something they cannot parse, and silently starting from `{}`
 * would throw away whatever the user was in the middle of typing. The caller
 * surfaces the parse error instead.
 */
export function applyClaudeConfig(
  settingsJson: string,
  patch: Partial<ClaudeConfigValue>
): ClaudeConfigWriteResult | null {
  const root = parseObject(settingsJson)
  if (!root) return null
  const env: Record<string, unknown> = { ...envBlock(root) }
  const columns: Partial<ClaudeProfileColumns> = {}

  if (patch.authMode === "official_subscription") {
    // Official means official: the file must not still carry an endpoint or a
    // token, or the "subscription" profile would quietly keep billing a
    // gateway. Clearing the columns alone is not enough — the file wins over
    // nothing here, it *is* the profile.
    for (const key of CONNECTION_ENV_KEYS) delete env[key]
    columns.baseUrl = ""
    columns.authToken = ""
  }
  if (patch.apiBaseUrl !== undefined) {
    columns.baseUrl = patch.apiBaseUrl
    setEnv(env, "ANTHROPIC_BASE_URL", patch.apiBaseUrl)
  }
  if (patch.apiKey !== undefined) {
    columns.authToken = patch.apiKey
    setEnv(env, "ANTHROPIC_AUTH_TOKEN", patch.apiKey)
  }
  if (patch.mainModel !== undefined) {
    columns.model = patch.mainModel
    setEnv(env, "ANTHROPIC_MODEL", patch.mainModel)
  }
  for (const [field, key] of Object.entries(ENV_KEYS)) {
    const value = patch[field as keyof typeof ENV_KEYS]
    if (value !== undefined) setEnv(env, key, value)
  }
  if (patch.sendAttributionHeader !== undefined) {
    env[ATTRIBUTION_HEADER_ENV_KEY] = patch.sendAttributionHeader ? "1" : "0"
  }
  if (patch.disableNonessentialTraffic !== undefined) {
    env[NONESSENTIAL_TRAFFIC_ENV_KEY] = patch.disableNonessentialTraffic
      ? "1"
      : "0"
  }

  const next: JsonObject = { ...root }
  if (patch.effortLevel !== undefined) {
    if (patch.effortLevel) next[EFFORT_LEVEL_KEY] = patch.effortLevel
    else delete next[EFFORT_LEVEL_KEY]
  }
  if (Object.keys(env).length > 0) next.env = env
  else delete next.env

  return { settingsJson: JSON.stringify(next, null, 2), columns }
}

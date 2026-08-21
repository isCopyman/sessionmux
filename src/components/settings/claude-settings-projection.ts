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
  type ClaudeAuthMode,
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

/**
 * The keys that decide where a session bills.
 *
 * All three, not just the two codeg writes: `ANTHROPIC_API_KEY` is the second
 * credential spelling the CLI accepts, so blanking only `AUTH_TOKEN` lets a
 * lower layer's key survive and keep authenticating.
 */
const CONNECTION_ENV_KEYS = [
  "ANTHROPIC_BASE_URL",
  "ANTHROPIC_AUTH_TOKEN",
  "ANTHROPIC_API_KEY",
] as const

function hasKey(bag: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(bag, key)
}

/**
 * Which of the three connection states this profile is in.
 *
 * Absent keys and blank keys are *different answers*, and reading them as the
 * same one is what made "official subscription" a label rather than a fact:
 * `--settings` is additive per key, so omitting `ANTHROPIC_BASE_URL` leaves the
 * project's `.claude/settings.json` in charge. Writing it as `""` is what
 * actually overrides the lower layers.
 */
function readAuthMode(
  env: Record<string, unknown>,
  columns: ClaudeProfileColumns
): ClaudeAuthMode {
  if (columns.baseUrl.trim() || columns.authToken.trim()) return "custom"
  if (CONNECTION_ENV_KEYS.some((key) => readString(env, key).trim())) {
    return "custom"
  }
  if (CONNECTION_ENV_KEYS.some((key) => hasKey(env, key))) {
    return "official_subscription"
  }
  return "inherit"
}

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
    authMode: readAuthMode(env, columns),
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
    // Blank, not deleted. Deleting only removes *this* profile's opinion, and
    // the project's `.claude/settings.json` then decides — which is how a
    // profile labelled "official subscription" kept billing a gateway. An
    // empty string wins the merge and the CLI reads it as unset, so the
    // session lands on the subscription login. Measured both ways in
    // CONFIG-MODEL-2026-08-21 §11.2.
    for (const key of CONNECTION_ENV_KEYS) env[key] = ""
    columns.baseUrl = ""
    columns.authToken = ""
  }
  if (patch.authMode === "inherit") {
    // The opposite choice, and a real one: say nothing, let the project and
    // user layers decide. That is what deleting the keys means.
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

/**
 * The columns a saved profile still needs, read back out of the file.
 *
 * The three columns are no longer a second place to edit a setting — the panel
 * has one editor now — but the backend still merges them last and the profile
 * list still shows the endpoint. Deriving them at save keeps them a projection
 * of the file instead of a rival store that can go stale and then silently win
 * at materialization.
 */
export function columnsFromSettings(
  settingsJson: string
): ClaudeProfileColumns {
  const env = envBlock(parseObject(settingsJson) ?? {})
  return {
    baseUrl: readString(env, "ANTHROPIC_BASE_URL").trim(),
    authToken: readString(env, "ANTHROPIC_AUTH_TOKEN").trim(),
    model: readString(env, "ANTHROPIC_MODEL").trim(),
  }
}

/**
 * The profile's *effective* settings, as one object: what the backend would
 * actually hand the CLI today.
 *
 * A profile still has three stores, and `materialize_managed_profile` merges
 * them in a fixed order — raw `settings_json` is the base, `record.env`
 * overlays it, and the three dedicated fields win last
 * (`src-tauri/src/commands/claude_profile.rs`). With the typed form gone the
 * editor has to show all three or it is lying, and the save path derives them
 * back out of what it shows.
 *
 * Reproducing that order exactly — later layers OVERWRITE, they do not defer to
 * a value already in the file — is what makes the round trip behaviour
 * preserving. Folding "only where absent" would leave the file's own value in
 * the text while the column was the one in force, and the next save, even a
 * rename, would silently switch the session to the other endpoint.
 *
 * Secrets fold in as the masks the API handed us; the backend maps an unchanged
 * mask back to the stored value. Unparseable text is returned untouched, since
 * losing what someone typed is worse than showing a stale layer.
 */
export function foldEffectiveSettings(
  settingsJson: string,
  recordEnv: Record<string, string> | undefined,
  columns: ClaudeProfileColumns
): string {
  const root = parseObject(settingsJson)
  if (!root) return settingsJson
  const env: Record<string, unknown> = { ...envBlock(root) }
  let changed = false
  const put = (key: string, value: string) => {
    // Empty layers are absent layers: the backend skips them with
    // `trim_non_empty`, so folding one would invent an override.
    if (!value.trim()) return
    if (readString(env, key) === value) return
    env[key] = value
    changed = true
  }
  for (const [key, value] of Object.entries(recordEnv ?? {})) put(key, value)
  put("ANTHROPIC_BASE_URL", columns.baseUrl)
  put("ANTHROPIC_AUTH_TOKEN", columns.authToken)
  put("ANTHROPIC_MODEL", columns.model)
  if (!changed) return settingsJson
  return JSON.stringify({ ...root, env }, null, 2)
}

/**
 * Drop credential-looking keys, for the "duplicate this profile" path.
 *
 * The endpoint is worth copying; the secret is not — the API only ever hands
 * the browser a mask, so carrying one over would show a row of bullets on a
 * profile that has no credential at all. The test mirrors the backend's own
 * `is_secret_env_key`, so a copy cannot keep bullets for some other vendor's
 * key just because this module only knew the two Anthropic spellings.
 */
export function stripCredentials(settingsJson: string): string {
  const root = parseObject(settingsJson)
  if (!root) return settingsJson
  const env: Record<string, unknown> = { ...envBlock(root) }
  const secret = (key: string) => /TOKEN|KEY|SECRET/.test(key.toUpperCase())
  const doomed = Object.keys(env).filter(secret)
  if (doomed.length === 0) return settingsJson
  for (const key of doomed) delete env[key]
  const next: JsonObject = { ...root }
  if (Object.keys(env).length > 0) next.env = env
  else delete next.env
  return JSON.stringify(next, null, 2)
}

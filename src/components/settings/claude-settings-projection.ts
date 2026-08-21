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
    const trimmed = value.trim()
    if (!trimmed) return
    if (readString(env, key) === trimmed) return
    env[key] = trimmed
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

import { describe, expect, it } from "vitest"

import {
  applyClaudeConfig,
  readClaudeConfig,
  type ClaudeProfileColumns,
} from "./claude-settings-projection"

const NO_COLUMNS: ClaudeProfileColumns = {
  baseUrl: "",
  authToken: "",
  model: "",
}

describe("readClaudeConfig", () => {
  it("reads an empty profile as a subscription profile", () => {
    const value = readClaudeConfig("", NO_COLUMNS)
    expect(value.authMode).toBe("official_subscription")
    expect(value.apiBaseUrl).toBe("")
    // Documented defaults, not "whatever false is".
    expect(value.sendAttributionHeader).toBe(false)
    expect(value.disableNonessentialTraffic).toBe(true)
  })

  it("reads model aliases and effort out of settings.json", () => {
    const text = JSON.stringify({
      effortLevel: "xhigh",
      env: {
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5",
        ANTHROPIC_REASONING_MODEL: "claude-opus-5",
      },
    })
    const value = readClaudeConfig(text, NO_COLUMNS)
    expect(value.haikuModel).toBe("claude-haiku-4-5")
    expect(value.opusModel).toBe("claude-opus-5")
    expect(value.reasoningModel).toBe("claude-opus-5")
    expect(value.effortLevel).toBe("xhigh")
  })

  it("collapses the 'default' effort sentinel to the empty placeholder", () => {
    const value = readClaudeConfig(
      JSON.stringify({ effortLevel: "default" }),
      NO_COLUMNS
    )
    expect(value.effortLevel).toBe("")
  })

  // The column carries the stored secret's mask, which is what the form shows.
  // Reading the mode off it directly is what lets an emptied field mean
  // "no credential" instead of "keep whatever you have".
  it("treats a masked token column as a real credential", () => {
    const value = readClaudeConfig("", {
      baseUrl: "",
      authToken: "sk-t••••••••7890",
      model: "",
    })
    expect(value.authMode).toBe("custom")
    expect(value.apiKey).toBe("sk-t••••••••7890")
  })

  it("reads back as a subscription once nothing is set", () => {
    const value = readClaudeConfig("", {
      baseUrl: "",
      authToken: "",
      model: "",
    })
    expect(value.authMode).toBe("official_subscription")
  })

  // A settings.json that authenticates with ANTHROPIC_API_KEY used to read
  // back as a subscription profile: the form then hid the connection fields on
  // a profile that was still sending a key.
  it("counts ANTHROPIC_API_KEY as a credential too", () => {
    const text = JSON.stringify({ env: { ANTHROPIC_API_KEY: "sk-ant-real" } })
    const value = readClaudeConfig(text, NO_COLUMNS)
    expect(value.authMode).toBe("custom")
    expect(value.apiKey).toBe("sk-ant-real")
  })

  it("prefers ANTHROPIC_AUTH_TOKEN when a file carries both", () => {
    const text = JSON.stringify({
      env: { ANTHROPIC_AUTH_TOKEN: "tok", ANTHROPIC_API_KEY: "key" },
    })
    expect(readClaudeConfig(text, NO_COLUMNS).apiKey).toBe("tok")
  })

  it("lets the dedicated columns win over the file, as the backend does", () => {
    const text = JSON.stringify({
      env: { ANTHROPIC_BASE_URL: "https://from-file.example" },
    })
    const value = readClaudeConfig(text, {
      ...NO_COLUMNS,
      baseUrl: "https://from-column.example",
    })
    expect(value.apiBaseUrl).toBe("https://from-column.example")
  })

  it("falls back to the file when a column is empty", () => {
    const text = JSON.stringify({
      env: { ANTHROPIC_BASE_URL: "https://from-file.example" },
    })
    expect(readClaudeConfig(text, NO_COLUMNS).apiBaseUrl).toBe(
      "https://from-file.example"
    )
  })

  it("reports a profile with an endpoint as a custom-endpoint profile", () => {
    const value = readClaudeConfig("", {
      ...NO_COLUMNS,
      authToken: "sk-test",
    })
    expect(value.authMode).toBe("custom")
  })

  it("survives unparseable text without blanking the columns", () => {
    const value = readClaudeConfig("{ not json", {
      ...NO_COLUMNS,
      baseUrl: "https://kept.example",
    })
    expect(value.apiBaseUrl).toBe("https://kept.example")
    expect(value.haikuModel).toBe("")
  })
})

describe("applyClaudeConfig", () => {
  it("writes an alias into the env block and keeps unrelated keys", () => {
    const before = JSON.stringify({
      permissions: { allow: ["Bash"] },
      env: { EXISTING: "keep" },
    })
    const result = applyClaudeConfig(before, { haikuModel: "claude-haiku-4-5" })
    expect(result).not.toBeNull()
    const parsed = JSON.parse(result!.settingsJson)
    expect(parsed.env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("claude-haiku-4-5")
    expect(parsed.env.EXISTING).toBe("keep")
    expect(parsed.permissions).toEqual({ allow: ["Bash"] })
  })

  it("deletes a key when the field is cleared rather than writing an empty string", () => {
    const before = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5" },
    })
    const result = applyClaudeConfig(before, { haikuModel: "" })
    const parsed = JSON.parse(result!.settingsJson)
    expect("ANTHROPIC_DEFAULT_HAIKU_MODEL" in (parsed.env ?? {})).toBe(false)
  })

  it("mirrors the endpoint into both the column and the file", () => {
    const result = applyClaudeConfig("{}", {
      apiBaseUrl: "https://gw.example",
    })
    expect(result!.columns.baseUrl).toBe("https://gw.example")
    expect(JSON.parse(result!.settingsJson).env.ANTHROPIC_BASE_URL).toBe(
      "https://gw.example"
    )
  })

  it("strips every connection key when the profile is switched to official", () => {
    const before = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://gw.example",
        ANTHROPIC_AUTH_TOKEN: "sk-test",
        ANTHROPIC_API_KEY: "sk-other",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5",
      },
    })
    const result = applyClaudeConfig(before, {
      authMode: "official_subscription",
    })
    const parsed = JSON.parse(result!.settingsJson)
    // Otherwise a "subscription" profile keeps billing the gateway.
    expect(parsed.env.ANTHROPIC_BASE_URL).toBeUndefined()
    expect(parsed.env.ANTHROPIC_AUTH_TOKEN).toBeUndefined()
    expect(parsed.env.ANTHROPIC_API_KEY).toBeUndefined()
    expect(result!.columns.baseUrl).toBe("")
    expect(result!.columns.authToken).toBe("")
    // Model choices are not connection state and must survive the switch.
    expect(parsed.env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("claude-opus-5")
  })

  it("writes effort at the top level and removes it for the default", () => {
    const on = applyClaudeConfig("{}", { effortLevel: "high" })
    expect(JSON.parse(on!.settingsJson).effortLevel).toBe("high")
    const off = applyClaudeConfig(on!.settingsJson, { effortLevel: "" })
    expect("effortLevel" in JSON.parse(off!.settingsJson)).toBe(false)
  })

  it("encodes the switches as the 1/0 the panel has always written", () => {
    const result = applyClaudeConfig("{}", {
      sendAttributionHeader: true,
      disableNonessentialTraffic: false,
    })
    const env = JSON.parse(result!.settingsJson).env
    expect(env.CLAUDE_CODE_ATTRIBUTION_HEADER).toBe("1")
    expect(env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC).toBe("0")
  })

  it("refuses to rewrite text it cannot parse", () => {
    expect(applyClaudeConfig("{ not json", { haikuModel: "x" })).toBeNull()
  })

  it("refuses a JSON value that is not an object", () => {
    expect(applyClaudeConfig("[1,2]", { haikuModel: "x" })).toBeNull()
  })

  it("round-trips through the reader", () => {
    const written = applyClaudeConfig("{}", {
      haikuModel: "claude-haiku-4-5",
      effortLevel: "medium",
      sendAttributionHeader: true,
    })
    const value = readClaudeConfig(written!.settingsJson, NO_COLUMNS)
    expect(value.haikuModel).toBe("claude-haiku-4-5")
    expect(value.effortLevel).toBe("medium")
    expect(value.sendAttributionHeader).toBe(true)
  })
})

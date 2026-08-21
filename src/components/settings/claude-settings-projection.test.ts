import { describe, expect, it } from "vitest"

import {
  applyClaudeConfig,
  columnsFromSettings,
  foldEffectiveSettings,
  readClaudeConfig,
  stripCredentials,
  type ClaudeProfileColumns,
} from "./claude-settings-projection"

const NO_COLUMNS: ClaudeProfileColumns = {
  baseUrl: "",
  authToken: "",
  model: "",
}

describe("readClaudeConfig", () => {
  // Not "official subscription": a profile that never mentions the connection
  // keys has not overridden anything, so the project's own .claude/settings.json
  // still decides where the session bills.
  it("reads an empty profile as inheriting, not as a subscription", () => {
    const value = readClaudeConfig("", NO_COLUMNS)
    expect(value.authMode).toBe("inherit")
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

  it("reads back as inheriting once nothing is set", () => {
    const value = readClaudeConfig("", {
      baseUrl: "",
      authToken: "",
      model: "",
    })
    expect(value.authMode).toBe("inherit")
  })

  // The distinction the old two-state read could not express, and the reason
  // "official subscription" used to be a label rather than a fact.
  it("tells a blanked key apart from a missing one", () => {
    const blanked = JSON.stringify({
      env: { ANTHROPIC_BASE_URL: "", ANTHROPIC_AUTH_TOKEN: "" },
    })
    expect(readClaudeConfig(blanked, NO_COLUMNS).authMode).toBe(
      "official_subscription"
    )

    const missing = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_OPUS_MODEL: "x" },
    })
    expect(readClaudeConfig(missing, NO_COLUMNS).authMode).toBe("inherit")
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

  const GATEWAY_PROFILE = JSON.stringify({
    env: {
      ANTHROPIC_BASE_URL: "https://gw.example",
      ANTHROPIC_AUTH_TOKEN: "sk-test",
      ANTHROPIC_API_KEY: "sk-other",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5",
    },
  })

  it("blanks every connection key when the profile is switched to official", () => {
    const result = applyClaudeConfig(GATEWAY_PROFILE, {
      authMode: "official_subscription",
    })
    const parsed = JSON.parse(result!.settingsJson)
    // Blank, not absent. Deleting the keys only withdraws this profile's
    // opinion and hands the decision to the project's .claude/settings.json,
    // which is how a "subscription" profile kept billing a gateway. An empty
    // string wins the merge and reads back as unset.
    expect(parsed.env.ANTHROPIC_BASE_URL).toBe("")
    expect(parsed.env.ANTHROPIC_AUTH_TOKEN).toBe("")
    expect(parsed.env.ANTHROPIC_API_KEY).toBe("")
    expect(result!.columns.baseUrl).toBe("")
    expect(result!.columns.authToken).toBe("")
    // Model choices are not connection state and must survive the switch.
    expect(parsed.env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("claude-opus-5")
  })

  it("removes the connection keys when the profile switches to inherit", () => {
    const result = applyClaudeConfig(GATEWAY_PROFILE, { authMode: "inherit" })
    const parsed = JSON.parse(result!.settingsJson)
    expect(parsed.env.ANTHROPIC_BASE_URL).toBeUndefined()
    expect(parsed.env.ANTHROPIC_AUTH_TOKEN).toBeUndefined()
    expect(parsed.env.ANTHROPIC_API_KEY).toBeUndefined()
    expect(result!.columns.baseUrl).toBe("")
    expect(result!.columns.authToken).toBe("")
    expect(parsed.env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("claude-opus-5")
  })

  // The two writes must be distinguishable by the reader, or the round trip
  // silently collapses them back into one state.
  it("round-trips both official and inherit", () => {
    for (const mode of ["official_subscription", "inherit"] as const) {
      const written = applyClaudeConfig(GATEWAY_PROFILE, { authMode: mode })
      const readBack = readClaudeConfig(written!.settingsJson, NO_COLUMNS)
      expect(readBack.authMode).toBe(mode)
    }
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

describe("foldEffectiveSettings", () => {
  const NO_ENV = undefined

  // The bug this rule exists to prevent: fold "only where absent" left the
  // file's value in the editor while the column was the one actually in force,
  // so the next save — even a rename — switched the session to the other
  // endpoint. Later layers overwrite, exactly as the backend merges them.
  it("lets a column beat a value already in the file, as the backend does", () => {
    const text = JSON.stringify({
      env: { ANTHROPIC_BASE_URL: "https://from-file.example" },
    })
    const folded = foldEffectiveSettings(text, NO_ENV, {
      baseUrl: "https://from-column.example",
      authToken: "",
      model: "",
    })
    expect(JSON.parse(folded).env.ANTHROPIC_BASE_URL).toBe(
      "https://from-column.example"
    )
  })

  it("layers record.env over the file and the columns over record.env", () => {
    const text = JSON.stringify({ env: { ANTHROPIC_MODEL: "from-file" } })
    const folded = foldEffectiveSettings(
      text,
      { ANTHROPIC_MODEL: "from-env", EXTRA: "kept" },
      { baseUrl: "", authToken: "", model: "from-column" }
    )
    const env = JSON.parse(folded).env
    expect(env.ANTHROPIC_MODEL).toBe("from-column")
    expect(env.EXTRA).toBe("kept")
  })

  // `trim_non_empty` skips blank layers on the backend, so folding one would
  // invent an override — and a blank connection key is a real instruction
  // ("force official") that an empty column must not be allowed to erase.
  it("ignores blank layers rather than inventing an override", () => {
    const text = JSON.stringify({ env: { ANTHROPIC_BASE_URL: "" } })
    const folded = foldEffectiveSettings(
      text,
      { ANTHROPIC_BASE_URL: "  " },
      { baseUrl: "", authToken: "", model: "" }
    )
    expect(JSON.parse(folded).env.ANTHROPIC_BASE_URL).toBe("")
  })

  it("returns unparseable text untouched instead of replacing it", () => {
    const broken = "{ not json"
    expect(
      foldEffectiveSettings(broken, NO_ENV, {
        baseUrl: "https://x.example",
        authToken: "",
        model: "",
      })
    ).toBe(broken)
  })

  // Fold then derive is the round trip the save path performs. It has to hand
  // back what was in force, or saving a rename would change where the session
  // bills.
  it("round-trips through columnsFromSettings", () => {
    const folded = foldEffectiveSettings("", NO_ENV, {
      baseUrl: "https://gw.example",
      authToken: "sk-t••••1234",
      model: "claude-opus-5",
    })
    expect(columnsFromSettings(folded)).toEqual({
      baseUrl: "https://gw.example",
      authToken: "sk-t••••1234",
      model: "claude-opus-5",
    })
  })
})

describe("stripCredentials", () => {
  it("drops every secret-looking key, not just the two Anthropic spellings", () => {
    const text = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://gw.example",
        ANTHROPIC_AUTH_TOKEN: "sk-a",
        ANTHROPIC_API_KEY: "sk-b",
        SOME_VENDOR_SECRET: "s",
        BUS_PROJECT: "kept",
      },
    })
    const env = JSON.parse(stripCredentials(text)).env
    expect(env).toEqual({
      ANTHROPIC_BASE_URL: "https://gw.example",
      BUS_PROJECT: "kept",
    })
  })
})

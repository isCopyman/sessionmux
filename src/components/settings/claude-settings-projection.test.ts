import { describe, expect, it } from "vitest"

import {
  columnsFromSettings,
  foldEffectiveSettings,
  stripCredentials,
  type ClaudeProfileColumns,
} from "./claude-settings-projection"

const NO_COLUMNS: ClaudeProfileColumns = {
  baseUrl: "",
  authToken: "",
  model: "",
}

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
      NO_COLUMNS
    )
    expect(JSON.parse(folded).env.ANTHROPIC_BASE_URL).toBe("")
  })

  it("trims values the same way as the backend materializer", () => {
    const folded = foldEffectiveSettings(
      "{}",
      { BUS_PROJECT: "  alpha  " },
      { ...NO_COLUMNS, model: "  claude-opus-5  " }
    )
    expect(JSON.parse(folded).env).toEqual({
      BUS_PROJECT: "alpha",
      ANTHROPIC_MODEL: "claude-opus-5",
    })
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

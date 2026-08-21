import { parseConfigJsonText } from "./shared"

export const CLINE_PROVIDERS = [
  { value: "anthropic", label: "Anthropic" },
  { value: "openai-native", label: "OpenAI" },
  { value: "openai", label: "OpenAI Compatible" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "gemini", label: "Gemini" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "bedrock", label: "AWS Bedrock" },
  { value: "vertex", label: "GCP Vertex" },
  { value: "ollama", label: "Ollama" },
] as const

export type ClineProvider = (typeof CLINE_PROVIDERS)[number]["value"]

export interface ClineImportantValues {
  provider: ClineProvider
  apiKey: string
  model: string
  baseUrl: string
}

export function extractClineImportantValues(
  configText: string
): ClineImportantValues {
  const parseResult = parseConfigJsonText(configText)
  const config = parseResult.config
  return {
    provider: (typeof config.apiProvider === "string" && config.apiProvider
      ? config.apiProvider
      : "anthropic") as ClineProvider,
    apiKey: typeof config.apiKey === "string" ? config.apiKey : "",
    model: typeof config.model === "string" ? config.model : "",
    baseUrl: typeof config.apiBaseUrl === "string" ? config.apiBaseUrl : "",
  }
}

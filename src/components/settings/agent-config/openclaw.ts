import { envFromConfig, findEnvValue, parseConfigJsonText } from "./shared"

export const OPENCLAW_ENV_KEYS = {
  gatewayUrl: "OPENCLAW_GATEWAY_URL",
  gatewayToken: "OPENCLAW_GATEWAY_TOKEN",
  sessionKey: "OPENCLAW_SESSION_KEY",
} as const

export interface OpenClawImportantValues {
  gatewayUrl: string
  gatewayToken: string
  sessionKey: string
}

export function extractOpenClawImportantValues(
  env: Record<string, string>,
  configText: string
): OpenClawImportantValues {
  const parseResult = parseConfigJsonText(configText)
  const config = parseResult.config
  const configEnv = envFromConfig(config)
  const mergedEnv = { ...env, ...configEnv }

  return {
    gatewayUrl: findEnvValue(mergedEnv, [OPENCLAW_ENV_KEYS.gatewayUrl]),
    gatewayToken: findEnvValue(mergedEnv, [OPENCLAW_ENV_KEYS.gatewayToken]),
    sessionKey: findEnvValue(mergedEnv, [OPENCLAW_ENV_KEYS.sessionKey]),
  }
}

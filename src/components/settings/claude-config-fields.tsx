"use client"

/**
 * The one form for "which Claude configuration is this".
 *
 * It is rendered twice: by the "Follow default" tab, where it edits the CLI's
 * own global config, and by every codeg profile tab, where it edits that
 * profile's `settings.json`. Only the store differs, and that is the caller's
 * business — the questions are identical either side.
 *
 * There used to be two of these, written weeks apart against different stores,
 * and they looked nothing alike: one had an auth mode, a provider picker, five
 * model fields and a JSON editor; the other had three inputs. A user looking at
 * both asked why "official login or API endpoint" was a choice on one tab and
 * not the other. It always was the same question. So it is now the same form,
 * and a field added here shows up on both tabs by construction.
 *
 * The niche keys (custom model option, the two traffic switches) sit behind one
 * disclosure rather than being dropped: they are rare enough that leading with
 * them is what made the panel unreadable, but removing controls people already
 * use is not a UI fix.
 */

import { useState } from "react"
import { useTranslations } from "next-intl"
import { ChevronRight, Eye, EyeOff } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { cn } from "@/lib/utils"

export const CLAUDE_AUTH_MODES = [
  "official_subscription",
  "custom",
  "model_provider",
] as const
export type ClaudeAuthMode = (typeof CLAUDE_AUTH_MODES)[number]

export type ClaudeEffortLevel = "" | "low" | "medium" | "high" | "xhigh"
export const CLAUDE_EFFORT_LEVEL_VALUES: ReadonlyArray<
  Exclude<ClaudeEffortLevel, "">
> = ["low", "medium", "high", "xhigh"]

/** Everything this form edits, flat. Callers map it onto their own store. */
export interface ClaudeConfigValue {
  authMode: ClaudeAuthMode
  apiBaseUrl: string
  apiKey: string
  mainModel: string
  reasoningModel: string
  haikuModel: string
  sonnetModel: string
  opusModel: string
  customModelOption: string
  customModelOptionName: string
  customModelOptionDescription: string
  effortLevel: ClaudeEffortLevel
  sendAttributionHeader: boolean
  disableNonessentialTraffic: boolean
}

export const EMPTY_CLAUDE_CONFIG_VALUE: ClaudeConfigValue = {
  authMode: "official_subscription",
  apiBaseUrl: "",
  apiKey: "",
  mainModel: "",
  reasoningModel: "",
  haikuModel: "",
  sonnetModel: "",
  opusModel: "",
  customModelOption: "",
  customModelOptionName: "",
  customModelOptionDescription: "",
  effortLevel: "",
  sendAttributionHeader: false,
  disableNonessentialTraffic: false,
}

export interface ClaudeModelProviderOption {
  id: number
  name: string
}

export interface ClaudeConfigFieldsProps {
  value: ClaudeConfigValue
  onChange: (patch: Partial<ClaudeConfigValue>) => void
  /**
   * Prefix for every `id` / `htmlFor` in here. Two instances can be mounted at
   * once (a profile tab under the CLI-global block), and duplicate ids would
   * make a label focus the wrong input.
   */
  idPrefix: string
  /**
   * Saved endpoints for the "model provider" auth mode. Omitted or empty drops
   * that mode from the picker: a profile has no provider binding, so offering
   * it there would be a dead option.
   */
  providers?: readonly ClaudeModelProviderOption[]
  providerId?: number | null
  onProviderSelect?: (id: string) => void
  /** Shown when the stored key only comes back masked, so blank means "keep". */
  apiKeyPlaceholder?: string
  /** Rendered under the API key, e.g. "copying a profile does not copy keys". */
  apiKeyHint?: React.ReactNode
}

export function ClaudeConfigFields({
  value,
  onChange,
  idPrefix,
  providers,
  providerId,
  onProviderSelect,
  apiKeyPlaceholder,
  apiKeyHint,
}: ClaudeConfigFieldsProps) {
  const t = useTranslations("AcpAgentSettings")
  const [keyVisible, setKeyVisible] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)

  const providerModeAvailable = (providers?.length ?? 0) > 0
  // A provider binding owns the endpoint and the models: the fields mirror it
  // rather than being editable, same as before the two forms were merged.
  const boundToProvider =
    providerModeAvailable && value.authMode === "model_provider"
  const showConnection = value.authMode !== "official_subscription"

  const modelField = (
    key: Extract<
      keyof ClaudeConfigValue,
      | "mainModel"
      | "reasoningModel"
      | "haikuModel"
      | "sonnetModel"
      | "opusModel"
    >,
    label: string,
    placeholder: string,
    wide = false
  ) => (
    <div className={cn("space-y-1.5", wide && "md:col-span-2")}>
      <label
        htmlFor={`${idPrefix}-${key}`}
        className="text-[11px] text-muted-foreground"
      >
        {label}
      </label>
      <Input
        id={`${idPrefix}-${key}`}
        value={value[key]}
        readOnly={boundToProvider}
        onChange={(event) => onChange({ [key]: event.target.value })}
        placeholder={placeholder}
      />
    </div>
  )

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label
          htmlFor={`${idPrefix}-auth-mode`}
          className="text-[11px] text-muted-foreground"
        >
          {t("claude.authMode")}
        </label>
        <Select
          value={value.authMode}
          onValueChange={(next) => {
            if (!CLAUDE_AUTH_MODES.includes(next as ClaudeAuthMode)) return
            onChange({ authMode: next as ClaudeAuthMode })
          }}
        >
          <SelectTrigger id={`${idPrefix}-auth-mode`} className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="start">
            <SelectItem value="official_subscription">
              {t("authModeOfficialSubscription")}
            </SelectItem>
            <SelectItem value="custom">
              {t("authModeCustomEndpoint")}
            </SelectItem>
            {providerModeAvailable ? (
              <SelectItem value="model_provider">
                {t("authModeModelProvider")}
              </SelectItem>
            ) : null}
          </SelectContent>
        </Select>
        <p className="text-[11px] text-muted-foreground">
          {value.authMode === "official_subscription"
            ? t("claude.officialSubscriptionHint")
            : value.authMode === "custom"
              ? t("authModeCustomEndpointHint")
              : t("modelProviderHint")}
        </p>
      </div>

      {providerModeAvailable && value.authMode === "model_provider" ? (
        <div className="space-y-1.5">
          <label
            htmlFor={`${idPrefix}-provider`}
            className="text-[11px] text-muted-foreground"
          >
            {t("selectModelProvider")}
          </label>
          <Select
            value={providerId != null ? String(providerId) : ""}
            onValueChange={(id) => onProviderSelect?.(id)}
          >
            <SelectTrigger id={`${idPrefix}-provider`} className="w-full">
              <SelectValue placeholder={t("selectModelProvider")} />
            </SelectTrigger>
            <SelectContent align="start">
              {providers?.map((provider) => (
                <SelectItem key={provider.id} value={String(provider.id)}>
                  {provider.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ) : null}

      {showConnection ? (
        <>
          <div className="space-y-1.5">
            <label
              htmlFor={`${idPrefix}-base-url`}
              className="text-[11px] text-muted-foreground"
            >
              API URL
            </label>
            <Input
              id={`${idPrefix}-base-url`}
              value={value.apiBaseUrl}
              readOnly={boundToProvider}
              onChange={(event) => onChange({ apiBaseUrl: event.target.value })}
              placeholder="https://api.example.com"
            />
          </div>

          <div className="space-y-1.5">
            <label
              htmlFor={`${idPrefix}-api-key`}
              className="text-[11px] text-muted-foreground"
            >
              API Key
            </label>
            <div className="flex items-center gap-2">
              <Input
                id={`${idPrefix}-api-key`}
                type={keyVisible ? "text" : "password"}
                value={value.apiKey}
                readOnly={boundToProvider}
                onChange={(event) => onChange({ apiKey: event.target.value })}
                placeholder={apiKeyPlaceholder ?? "sk-..."}
                autoComplete="off"
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setKeyVisible((shown) => !shown)}
                title={
                  keyVisible ? t("actions.hideApiKey") : t("actions.showApiKey")
                }
              >
                {keyVisible ? (
                  <EyeOff className="h-3.5 w-3.5" />
                ) : (
                  <Eye className="h-3.5 w-3.5" />
                )}
              </Button>
            </div>
            {apiKeyHint}
          </div>
        </>
      ) : null}

      <div className="space-y-2">
        <div className="grid gap-3 md:grid-cols-2">
          {modelField("mainModel", t("claude.mainModel"), "claude-sonnet-5")}
          {modelField(
            "reasoningModel",
            t("claude.reasoningModel"),
            "claude-opus-5"
          )}
          {modelField(
            "haikuModel",
            t("claude.haikuDefaultModel"),
            "claude-haiku-4-5"
          )}
          {modelField(
            "sonnetModel",
            t("claude.sonnetDefaultModel"),
            "claude-sonnet-5"
          )}
          {modelField(
            "opusModel",
            t("claude.opusDefaultModel"),
            "claude-opus-5",
            true
          )}
        </div>
        <p className="text-[11px] text-muted-foreground">
          {t("modelHintDefault")}
        </p>
      </div>

      <div className="space-y-1.5">
        <label
          htmlFor={`${idPrefix}-effort`}
          className="text-[11px] text-muted-foreground"
        >
          {t("claude.effortLevel")}
        </label>
        <Select
          value={value.effortLevel || "default"}
          onValueChange={(next) =>
            onChange({
              effortLevel:
                next === "default" ? "" : (next as ClaudeEffortLevel),
            })
          }
        >
          <SelectTrigger id={`${idPrefix}-effort`} className="w-full">
            <SelectValue placeholder={t("claude.effortLevelDefault")} />
          </SelectTrigger>
          <SelectContent align="start">
            <SelectItem value="default">
              {t("claude.effortLevelDefault")}
            </SelectItem>
            {CLAUDE_EFFORT_LEVEL_VALUES.map((level) => (
              <SelectItem key={level} value={level}>
                {t(`claude.effortLevel_${level}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <button
          type="button"
          className="flex w-full items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
        >
          <ChevronRight
            className={cn(
              "h-3 w-3 transition-transform",
              advancedOpen && "rotate-90"
            )}
          />
          {t("claude.advancedFields")}
        </button>
        {advancedOpen ? (
          <div className="space-y-2 border-t border-border/60 pt-3">
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1.5 md:col-span-2">
                <label
                  htmlFor={`${idPrefix}-custom-model`}
                  className="text-[11px] text-muted-foreground"
                >
                  {t("claude.customModelOption")}
                </label>
                <Input
                  id={`${idPrefix}-custom-model`}
                  value={value.customModelOption}
                  readOnly={boundToProvider}
                  onChange={(event) =>
                    onChange({ customModelOption: event.target.value })
                  }
                  placeholder="my-gateway/claude-opus-5"
                />
              </div>
              <div className="space-y-1.5">
                <label
                  htmlFor={`${idPrefix}-custom-model-name`}
                  className="text-[11px] text-muted-foreground"
                >
                  {t("claude.customModelOptionName")}
                </label>
                <Input
                  id={`${idPrefix}-custom-model-name`}
                  value={value.customModelOptionName}
                  readOnly={boundToProvider}
                  onChange={(event) =>
                    onChange({ customModelOptionName: event.target.value })
                  }
                  placeholder="Gateway Opus"
                />
              </div>
              <div className="space-y-1.5">
                <label
                  htmlFor={`${idPrefix}-custom-model-desc`}
                  className="text-[11px] text-muted-foreground"
                >
                  {t("claude.customModelOptionDescription")}
                </label>
                <Input
                  id={`${idPrefix}-custom-model-desc`}
                  value={value.customModelOptionDescription}
                  readOnly={boundToProvider}
                  onChange={(event) =>
                    onChange({
                      customModelOptionDescription: event.target.value,
                    })
                  }
                  placeholder="Routed via custom gateway"
                />
              </div>
            </div>
            <p className="text-[11px] text-muted-foreground">
              {t("claude.customModelOptionHint")}
            </p>

            <div className="flex items-center justify-between rounded-md border px-3 py-2">
              <label className="text-[11px] text-muted-foreground">
                {t("claude.sendAttributionHeader")}
              </label>
              <Switch
                checked={value.sendAttributionHeader}
                onCheckedChange={(checked) =>
                  onChange({ sendAttributionHeader: checked })
                }
                aria-label={t("claude.sendAttributionHeaderAria")}
              />
            </div>
            <div className="flex items-center justify-between rounded-md border px-3 py-2">
              <label className="text-[11px] text-muted-foreground">
                {t("claude.disableNonessentialTraffic")}
              </label>
              <Switch
                checked={value.disableNonessentialTraffic}
                onCheckedChange={(checked) =>
                  onChange({ disableNonessentialTraffic: checked })
                }
                aria-label={t("claude.disableNonessentialTrafficAria")}
              />
            </div>
          </div>
        ) : null}
      </div>
    </div>
  )
}

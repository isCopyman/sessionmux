/**
 * Harness-neutral entrypoint for Session launch Profile controls.
 *
 * The implementation still re-exports its original Claude filename so older
 * imports remain source-compatible while new surfaces depend on the generic
 * capability rather than a provider name.
 */
export {
  AgentProfileSelectorDropdown,
  InlineAgentProfileSelector,
  useAgentProfileSelectorModel,
  type AgentProfileSelectorModel,
  type AgentProfileSelectorOption,
  type AgentProfileSelectorProps,
} from "./claude-profile-selector"

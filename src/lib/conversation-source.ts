import type { ConversationCreatedBy, DbConversationSummary } from "@/lib/types"

/** The only field the source facets read, so callers can pass a summary, a
 *  partial row from a store, or a test fixture without widening to the full
 *  {@link DbConversationSummary}. */
type SourceBearing = Pick<DbConversationSummary, "created_by">

const KNOWN_SOURCES: readonly ConversationCreatedBy[] = [
  "user",
  "agent",
  "automation",
]

/**
 * Who a Session reads as having been started by.
 *
 * `created_by` is absent on rows written before the column existed, and an
 * older server can omit it from the list payload entirely — both resolve to
 * `user`, which is what those Sessions in fact were: nothing else could create
 * one at the time. A value outside the known set (a newer server sending a
 * source this build has no UI for) resolves the same way, so an unrecognized
 * Session stays visible in the default view rather than silently vanishing
 * behind every filter.
 */
export function conversationSource(
  conversation: SourceBearing
): ConversationCreatedBy {
  const value = conversation.created_by
  return value != null && KNOWN_SOURCES.includes(value) ? value : "user"
}

/** The Session Center facet: one source, or every source. */
export type SessionSourceFilter = "all" | ConversationCreatedBy

export function matchesSessionSource(
  conversation: SourceBearing,
  filter: SessionSourceFilter
): boolean {
  return filter === "all" || conversationSource(conversation) === filter
}

/**
 * The sidebar's form of the same facet: two independent hide switches rather
 * than a one-of-N dropdown, matching the funnel menu's other options. There is
 * deliberately no switch for user-created Sessions — "hide the ones I started"
 * is not a view anyone asked for, and leaving it out keeps the menu from
 * offering a combination that empties the whole list.
 */
export interface SessionSourceVisibility {
  showAgentCreated: boolean
  showAutomationCreated: boolean
}

export const ALL_SESSION_SOURCES_VISIBLE: SessionSourceVisibility = {
  showAgentCreated: true,
  showAutomationCreated: true,
}

export function isSessionSourceVisible(
  conversation: SourceBearing,
  visibility: SessionSourceVisibility
): boolean {
  switch (conversationSource(conversation)) {
    case "agent":
      return visibility.showAgentCreated
    case "automation":
      return visibility.showAutomationCreated
    case "user":
      return true
  }
}

import { formatConversationTitle } from "@/lib/conversation-title"
import type { DbConversationSummary } from "@/lib/types"

/**
 * Sessions eligible to become Room members: anything not archived, optionally
 * narrowed by a title/id search. Mirrors the Rooms page's add-member
 * candidate list.
 */
export function roomMemberCandidates(
  conversations: readonly DbConversationSummary[],
  query: string
): DbConversationSummary[] {
  const needle = query.trim().toLowerCase()
  return conversations
    .filter((conversation) => conversation.archived_at == null)
    .filter((conversation) => {
      if (!needle) return true
      const title = formatConversationTitle(conversation.title).toLowerCase()
      return title.includes(needle) || String(conversation.id).includes(needle)
    })
}

/**
 * Default Room title from the member picks: the first two titles joined, with
 * an ellipsis when more Sessions are in. Same rule as the bulk action bar's
 * create-room flow.
 */
export function defaultRoomTitle(
  members: readonly Pick<DbConversationSummary, "title">[],
  fallback: string
): string {
  const title =
    members
      .slice(0, 2)
      .map((member) => formatConversationTitle(member.title))
      .filter(Boolean)
      .join(" / ") || fallback
  return members.length > 2 ? `${title}…` : title
}

import type {
  ReferenceSearch,
  SuggestionGroup,
} from "@/components/chat/composer/suggestion/types"
import type { ReferenceAttrs } from "@/components/chat/composer/types"
import { formatConversationTitle } from "@/lib/conversation-title"
import { rankByTextMatch } from "@/lib/fuzzy-text-match"
import type { AgentType, CollaborationRoomMember } from "@/lib/types"

/** The member fields the session group needs — a subset of
 * {@link CollaborationRoomMember} so tests can pass a lighter shape. */
export type RoomSessionMember = Pick<
  CollaborationRoomMember,
  "conversationId" | "title" | "agentType"
>

/**
 * Localized strings the room's `@` search needs, resolved by the caller
 * (kept as plain values rather than a `t` function so this module stays
 * decoupled from next-intl — the same convention `room-message-body.ts` uses
 * for its `allLabel`/`humanLabel`/`untitled` parameters).
 */
export interface RoomMentionLabels {
  /** Heading for the room's own session group (the member roster). */
  sessionGroupLabel: string
  /** Label for the structured "wake everyone" pseudo-mention. */
  allLabel: string
  /** Label for the structured "wake the human" pseudo-mention. */
  humanLabel: string
  /** Fallback display name for a member with no title. */
  untitled: (id: number) => string
}

function memberLabel(
  member: Pick<RoomSessionMember, "conversationId" | "title">,
  untitled: (id: number) => string
): string {
  const title = member.title?.trim()
  return title
    ? formatConversationTitle(title)
    : untitled(member.conversationId)
}

/**
 * Builds the room's session group: the two structured pseudo-mentions
 * (@all / @human) plus one row per room member, ranked against `query`. This
 * is ALWAYS the room panel's session tab — the generic, workspace-wide
 * session group a plain {@link ReferenceSearch} would return is dropped in
 * favor of it (see {@link combineRoomMentionGroups}), since a room can only
 * wake its own members.
 */
export function buildRoomSessionGroup(
  query: string,
  members: readonly RoomSessionMember[],
  labels: Pick<
    RoomMentionLabels,
    "sessionGroupLabel" | "allLabel" | "humanLabel" | "untitled"
  >
): SuggestionGroup {
  const pseudo: { reference: ReferenceAttrs; keywords: string }[] = [
    {
      reference: {
        refType: "session",
        id: "all",
        label: labels.allLabel,
        uri: "codeg://all",
        meta: null,
      },
      keywords: "all everyone 全体 全體",
    },
    {
      reference: {
        refType: "session",
        id: "human",
        label: labels.humanLabel,
        uri: "codeg://human",
        meta: null,
      },
      keywords: "human user 人类 人類",
    },
  ]
  const memberItems = members.map((member) => ({
    reference: {
      refType: "session" as const,
      id: String(member.conversationId),
      label: memberLabel(member, labels.untitled),
      uri: `codeg://session/${member.conversationId}`,
      meta: member.agentType
        ? { agentType: member.agentType as AgentType }
        : null,
    },
    keywords: String(member.conversationId),
  }))
  const all = [...pseudo, ...memberItems]
  return {
    kind: "session",
    label: labels.sessionGroupLabel,
    items: rankByTextMatch(
      query,
      all,
      (item) => item.reference.label,
      (item) => item.keywords
    ),
  }
}

/**
 * Folds the room's session group together with the file/commit groups from a
 * standard workspace search (`useReferenceSearch`). Agent and the search's
 * own (workspace-wide) session group are dropped — a room's `@` only ever
 * offers its own members for waking, and agents have no wake semantics
 * inside a room. Pure: takes the already-resolved search result rather than
 * calling a `ReferenceSearch` itself, so the merge is testable without an
 * async round-trip.
 */
export function combineRoomMentionGroups(
  sessionGroup: SuggestionGroup,
  searchGroups: readonly SuggestionGroup[]
): SuggestionGroup[] {
  return [
    sessionGroup,
    ...searchGroups.filter(
      (group) => group.kind === "file" || group.kind === "commit"
    ),
  ]
}

/**
 * Bounds how long the room's `@` search waits on the workspace-wide
 * file/commit lookup. A wedged git-log / file-tree / session-list fetch (the
 * underlying hang is tracked separately) must never leave the room's panel
 * stuck on "searching…" forever — past this budget we degrade exactly like
 * a throwing/rejecting search would. The race below never cancels the
 * underlying call; a result that arrives after the deadline just resolves
 * into nothing, unused.
 */
const REFERENCE_SEARCH_TIMEOUT_MS = 3000

/**
 * Builds the Room composer's `@` search: the member roster (+ @all/@human)
 * always first, then the file/commit groups from a workspace-wide search
 * scoped to the room's resolved folder — empty (not missing) when the room
 * has no resolvable folder, since the underlying search still returns every
 * group shape with empty items rather than omitting them. See
 * `RoomWorkspace` for how the folder is resolved (never falling back to the
 * currently-active folder — a room is a cross-folder concept). If the
 * underlying search itself throws, rejects, or simply never settles, the
 * file/commit groups are dropped instead — the member roster must never
 * disappear just because a git-log / file-tree fetch failed or hung.
 */
export function buildRoomMentionSearch(
  members: readonly RoomSessionMember[],
  labels: RoomMentionLabels,
  referenceSearch: ReferenceSearch
): ReferenceSearch {
  return async (query, signal) => {
    const sessionGroup = buildRoomSessionGroup(query, members, labels)
    // A failure — or a hang — in the workspace-wide file/commit lookup (a
    // throwing, rejecting, or wedged git-log / file-tree / session-list
    // fetch) must never take the room's own member roster down with it —
    // degrade to just the session group rather than losing
    // @all/@human/members too.
    let searchGroups: SuggestionGroup[]
    try {
      searchGroups = await Promise.race([
        referenceSearch(query, signal),
        new Promise<SuggestionGroup[]>((resolve) => {
          setTimeout(() => resolve([]), REFERENCE_SEARCH_TIMEOUT_MS)
        }),
      ])
    } catch {
      searchGroups = []
    }
    return combineRoomMentionGroups(sessionGroup, searchGroups)
  }
}

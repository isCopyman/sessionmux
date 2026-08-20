"use client"

import type { ComponentProps } from "react"
import { useMemo } from "react"
import remarkBreaks from "remark-breaks"
import { Streamdown, type Components } from "streamdown"

import { MarkdownLink, nodeText } from "@/components/ai-elements/markdown-link"
import {
  markdownRehypePlugins,
  markdownRemarkPlugins,
} from "@/components/ai-elements/message"
import { useStreamdownPlugins } from "@/components/ai-elements/streamdown-plugins"
import { ReferenceBadge } from "@/components/chat/composer/badges/reference-badge"

// The three destinations `roomMessageMarkdown` emits for a chip, matched after
// the href has been through sanitize/harden — hence the tolerated trailing
// slash, which uri normalization may add to an authority-only url. Session ids
// are numeric here (a Room roster is always current-backend conversations),
// unlike the legacy `codeg://session/<agent>_<external>` form the Session
// transcript still has to accept.
const SESSION_HREF = /^codeg:\/\/session\/(\d+)\/?$/i
const ALL_HREF = /^codeg:\/\/all\/?$/i
const HUMAN_HREF = /^codeg:\/\/human\/?$/i

const MENTION_CLASS =
  "rounded-[3px] bg-sky-500/15 px-0.5 font-medium text-sky-800 dark:bg-sky-400/20 dark:text-sky-200"

type RemarkPlugins = NonNullable<
  ComponentProps<typeof Streamdown>["remarkPlugins"]
>

// A soft line break is a real line break in a chat channel: a Room post is
// written in a chat composer, and before Markdown rendering every newline was
// visible (`whitespace-pre-wrap`). Appended AFTER the shared list so
// remarkRestoreWindowsPaths still sees whole text nodes.
const roomRemarkPlugins: RemarkPlugins = [
  ...markdownRemarkPlugins,
  remarkBreaks,
]

type RoomLinkProps = ComponentProps<"a"> & {
  // react-markdown passes the originating hast node; it must not reach the DOM.
  node?: unknown
  onOpenSession?: (conversationId: number) => void
}

/**
 * Anchor renderer for Room posts: the three room mention destinations become
 * the chips the timeline had before Markdown rendering, everything else (file /
 * commit badges, http links and their safety routing) falls through to the
 * shared {@link MarkdownLink}.
 */
function RoomLink({
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  node,
  href,
  children,
  onOpenSession,
  ...rest
}: RoomLinkProps) {
  const uri = href ?? ""
  const session = SESSION_HREF.exec(uri)
  if (session) {
    const label = nodeText(children)
    // Same badge the Session transcript gives a `codeg://session/<id>` link,
    // but wrapped in a button: in a Room the chip is the way to reach the
    // Session that owns the work being discussed.
    const badge = (
      <ReferenceBadge
        data={{
          refType: "session",
          id: session[1],
          label,
          uri,
          meta: null,
        }}
      />
    )
    if (!onOpenSession) return badge
    const conversationId = Number(session[1])
    return (
      <button
        type="button"
        className="cursor-pointer"
        onClick={() => onOpenSession(conversationId)}
      >
        {badge}
      </button>
    )
  }
  if (ALL_HREF.test(uri) || HUMAN_HREF.test(uri)) {
    return <span className={MENTION_CLASS}>{nodeText(children)}</span>
  }
  return (
    <MarkdownLink href={href} {...rest}>
      {children}
    </MarkdownLink>
  )
}

/**
 * A Room post's body, rendered as Markdown by the shared Streamdown pipeline
 * (`source` comes from `roomMessageMarkdown`).
 *
 * Deliberately NOT done here: the reply-artifact treatment a Session reply gets
 * (file-change lists, diff cards). A Room is a coordination channel — the heavy
 * artifact stays in the Session that produced it, a ```diff fence renders as an
 * ordinary code block, and the chip is the way to go read the full change.
 *
 * `normalizeMathDelimiters` is also deliberately skipped: `source` carries
 * backslash-escaped chip labels, and a Session title like `a(b)` escapes to
 * `a\(b\)` — which that rewrite would turn into display math.
 */
export function RoomPostBody({
  source,
  onOpenSession,
}: {
  source: string
  onOpenSession?: (conversationId: number) => void
}) {
  const plugins = useStreamdownPlugins(source)
  const components = useMemo<Components>(
    () => ({
      // react-markdown's `Components` map carries a string index signature that
      // widens every override to `Record<string, unknown>` props; the cast is
      // the same bridge `markdownLinkComponents` uses.
      a: ((props: RoomLinkProps) => (
        <RoomLink {...props} onOpenSession={onOpenSession} />
      )) as Components["a"],
    }),
    [onOpenSession]
  )

  return (
    <div
      data-room-markdown=""
      // What Ctrl+F searches inside a post row — the body only, never the
      // author line or the quoted-post locator above it.
      data-conversation-search-content=""
      className="min-w-0 text-[15px] leading-6 text-foreground"
    >
      <Streamdown
        className="space-y-2"
        plugins={plugins}
        remarkPlugins={roomRemarkPlugins}
        rehypePlugins={markdownRehypePlugins}
        components={components}
      >
        {source}
      </Streamdown>
    </div>
  )
}

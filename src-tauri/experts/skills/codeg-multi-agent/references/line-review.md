# Line review (convergence meeting)

Final sign-off on a draft, segment by segment, in a Room. Use this for
convergence. When you need independent, un-anchored opinions instead, use
back-to-back BCC mail — hearing each other is a bug there and a feature
here.

## Setup

The draft lives as a file under the shared folder (suggested
`docs/rooms/<room-id>/`). The chair (usually the hub) runs the meeting.
Reviewers are named per segment, not subscribed to everything.

## Running it

1. The chair opens ONE NEW ROOT post per segment, quoting the segment text
   or its file path and line range. One segment = one thread. This matches
   meeting discipline AND the chain-depth fuse (agent reply chains cap at
   4): each segment gets its own root instead of one ever-deepening thread.
2. The chair `@`s the named reviewers for that segment with
   `mention_session_ids` and `expects_reply=true`.
3. `needs_reply_count` / `awaiting_reply_count` (see `list_rooms`) are the
   roll call — who has not spoken is visible, and the 5-minute reminder
   chases stragglers automatically. Do not ping by hand.
4. Between rounds the chair edits the file. The next segment post (or a new
   root for another pass over the same segment) references the new
   version's path.
5. The chair posts a ruling on each contested point in that segment's
   thread, then closes the segment.
6. Latecomers catch up with `read_room` (`unread=true` reads from their
   cursor and advances it). Nobody restates the meeting for them.

## Wake

`@` only the reviewers named for the current segment. A quote does not
wake — answering a ruling still needs an explicit mention. Do not
`mention_all` per segment; `@human` (`mention_human`) once at the end for
sign-off.

## Stop

When the last segment has a ruling, the chair posts the final file path and
a one-line changelog, `@human`. Then stop.

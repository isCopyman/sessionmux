/**
 * Whether a fork-and-send must be blocked because the message queue is
 * non-empty. Fork is an immediate session side effect (it re-points the live
 * session), so it cannot run while drafts are queued for the CURRENT session —
 * the queued items would otherwise flush onto the forked session, i.e. the fork
 * would jump ahead of the queue. The user drains/clears the queue first.
 */
export function forkSendBlockedByQueue(queueLength: number): boolean {
  return queueLength > 0
}

/**
 * Whether the live connection is ready to accept a send for THIS tab: connected
 * AND its established cwd matches the tab's intended working dir.
 *
 * Bare `connStatus === "connected"` is insufficient. A chat draft that just
 * retargeted into folderless mode (or any tab mid-reconnect) can read a stale
 * "connected" belonging to the PREVIOUS cwd for a render or two before the
 * reconnect lands. Sending then would deliver the prompt to the wrong
 * agent/workspace. Both the direct send and the backend queue worker enforce
 * this boundary. Nullish cwds are normalized so `null`/`undefined` compare
 * equal (both mean "no cwd yet").
 */
export function isConnectionReady(
  connStatus: string | null | undefined,
  connectedWorkingDir: string | null | undefined,
  intendedWorkingDir: string | null | undefined
): boolean {
  return (
    connStatus === "connected" &&
    (connectedWorkingDir ?? null) === (intendedWorkingDir ?? null)
  )
}

/**
 * Whether a direct submit must be rejected because an unbound new-tab create is
 * already in flight (single-flight the create).
 *
 * A second submit fired before the first create resolves (double Enter / double
 * click) would otherwise append an optimistic turn it can never deliver: the
 * create-pending guard that actually stops the duplicate returns only AFTER the
 * optimistic append. `hasPersistedId` is true once the tab is bound to a real
 * conversation row, so only the unbound path is single-flighted — persisted
 * conversations keep their legitimate concurrent queued-send behavior.
 */
export function shouldRejectDuplicateCreate(
  hasPersistedId: boolean,
  createPending: boolean
): boolean {
  return !hasPersistedId && createPending
}

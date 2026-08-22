/**
 * "Turn this message into a work task" hand-off. The Tasks route page is NOT
 * kept mounted (WorkbenchRoutePage renders only the active route), so a plain
 * CustomEvent would race the mount — the draft is therefore parked in a
 * module-level buffer that the page consumes on mount, with the event as the
 * already-mounted fast path. Mirrors the session-attachment-events idiom.
 */

export const CREATE_TASK_FROM_TEXT_EVENT = "codeg:create-task-from-text"
export const OPEN_TASK_DETAIL_EVENT = "codeg:open-task-detail"

export interface CreateTaskFromTextDetail {
  /** Message text to seed the task description with. */
  text: string
  /** The source conversation's project folder, when known. */
  folderId: number | null
}

let pendingDraft: CreateTaskFromTextDetail | null = null
let pendingTaskDetailId: number | null = null

/** Park a draft and nudge a mounted Tasks page (the caller switches routes). */
export function requestCreateTaskFromText(detail: CreateTaskFromTextDetail) {
  pendingDraft = detail
  window.dispatchEvent(new CustomEvent(CREATE_TASK_FROM_TEXT_EVENT))
}

/** One-shot consume — called by the Tasks page on mount and on the event. */
export function consumePendingTaskDraft(): CreateTaskFromTextDetail | null {
  const draft = pendingDraft
  pendingDraft = null
  return draft
}

/** Park a task selection before switching to the lazily-mounted Tasks page. */
export function requestOpenTaskDetail(taskId: number) {
  pendingTaskDetailId = taskId
  window.dispatchEvent(new CustomEvent(OPEN_TASK_DETAIL_EVENT))
}

/** One-shot consume — the task page opens the same card, never a copied view. */
export function consumePendingTaskDetail(): number | null {
  const taskId = pendingTaskDetailId
  pendingTaskDetailId = null
  return taskId
}

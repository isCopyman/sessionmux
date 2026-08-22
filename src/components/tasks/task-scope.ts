import type { TasksScope } from "@/lib/tasks-board-filter-storage"
import type { WorkTask } from "@/lib/types"

/** Multica-like quick views over the same task rows. These are deliberately
 * overlapping filters rather than new boards or persisted task states. */
export function filterTasksByScope(
  tasks: readonly WorkTask[],
  scope: TasksScope
): WorkTask[] {
  if (scope === "all") return [...tasks]
  return tasks.filter((task) => taskMatchesScope(task, scope))
}

export function taskMatchesScope(task: WorkTask, scope: TasksScope): boolean {
  switch (scope) {
    case "all":
      return true
    case "agent":
      return (
        task.execution_mode === "session" || task.execution_mode === "engine"
      )
    case "attention":
      return task.task_status === "review" || task.task_status === "blocked"
  }
}

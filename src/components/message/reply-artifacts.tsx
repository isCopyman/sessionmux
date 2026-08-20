"use client"

import { memo, useMemo, useState } from "react"
import {
  ChevronRight,
  ExternalLink,
  FileDiff,
  FileIcon,
  FilePlus,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { useActiveFolder } from "@/contexts/active-folder-context"
import { useWorkspaceActions } from "@/contexts/workspace-context"
import {
  CommitFileAdditions,
  CommitFileDeletions,
} from "@/components/ai-elements/commit"
import { UnifiedDiffPreview } from "@/components/diff/unified-diff-preview"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  fileNameOf,
  isAddedFileDiff,
  isRemovedFileDiff,
  normalizeSlashPath,
  toAbsoluteFilePath,
  toFolderRelativePath,
} from "@/lib/file-path-display"
import {
  extractReplyFileChanges,
  type FileChangeStat,
} from "@/lib/session-files"
import { isLocalDesktop, revealItemInDir } from "@/lib/platform"
import type { MessageTurn } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Per-file switch that opens the section's inline diff panel. It doubles as
 * the card's +N/-N badge so the stats themselves are the (large) hit target,
 * with the `FileDiff` glyph marking what a click does. Disabled — with an
 * explanatory tooltip — when the reply captured no diff text for the file.
 */
function InlineDiffToggle({
  file,
  expanded,
  accent,
  onToggle,
}: {
  file: FileChangeStat
  expanded: boolean
  accent: "green" | "neutral"
  onToggle: () => void
}) {
  const t = useTranslations("Folder.chat.replyArtifacts")
  const hasDiff = Boolean(file.diff)
  const label = !hasDiff
    ? t("noInlineDiff")
    : expanded
      ? t("hideInlineDiff")
      : t("showInlineDiff")

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/* Wrapper span so the tooltip still fires while the button is
            disabled (disabled elements don't emit pointer events). */}
        <span className={cn("flex shrink-0", !hasDiff && "cursor-not-allowed")}>
          <button
            type="button"
            onClick={onToggle}
            disabled={!hasDiff}
            aria-expanded={expanded}
            aria-label={label}
            className={cn(
              "flex shrink-0 items-center gap-1 border-l px-2 font-mono text-[10px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40",
              accent === "green"
                ? "border-green-600/30 hover:bg-green-500/15 dark:border-green-400/30"
                : "border-border hover:bg-accent/60",
              expanded &&
                (accent === "green" ? "bg-green-500/15" : "bg-accent/60")
            )}
          >
            <CommitFileAdditions
              count={file.additions}
              className="text-[10px]"
            />
            <CommitFileDeletions
              count={file.deletions}
              className="text-[10px]"
            />
            <FileDiff
              className={cn(
                "h-3.5 w-3.5 shrink-0",
                expanded ? "text-foreground" : "text-muted-foreground"
              )}
            />
          </button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  )
}

/**
 * Full-width inline diff for the one file expanded within a section. Rendered
 * below the card grid rather than inside a card, so a diff always gets the
 * whole container width instead of one narrow grid column.
 */
function InlineDiffPanel({
  diff,
  displayPath,
}: {
  diff: string
  displayPath: string
}) {
  return (
    <div className="border-t border-border bg-background/50">
      <div className="flex items-center gap-2 px-3 py-1.5">
        <FileDiff className="h-3 w-3 shrink-0 text-muted-foreground" />
        <span
          className="min-w-0 truncate font-mono text-[10px] text-muted-foreground"
          title={displayPath}
        >
          {displayPath}
        </span>
      </div>
      {/* `unbounded` hands the vertical cap to this box — the preview would
          otherwise nest its own 420px scroll box inside ours — while each file
          keeps scrolling HORIZONTALLY on its own, so a long line can never
          push a horizontal scrollbar onto the page. `embedded` drops the
          preview's own border + path header, which the card already supplies. */}
      <div className="max-h-96 overflow-y-auto border-t border-border/50">
        <UnifiedDiffPreview diffText={diff} embedded unbounded />
      </div>
    </div>
  )
}

/**
 * Inline "artifacts" card shown at the end of a completed assistant reply
 * (above the `TurnStats` action row inside `HistoricalMessageGroup`).
 *
 * Two independently-collapsible sections:
 *  - "New files": every file the reply created, each as its own card in a
 *    container-responsive grid. The card body opens the file in the workspace
 *    tabs (an "open in editor" tooltip on hover); a distinct side button
 *    reveals it in the OS file manager. Open by default — a freshly written
 *    file is usually the thing you want to jump into. The grid scrolls
 *    within the same bounded max-height as the changed list.
 *  - "Files changed": modified/removed files, each rendered as its own card in
 *    the same responsive grid as "New files". A modified file's card opens the
 *    file in the workspace tabs on click (with a view-diff and a
 *    reveal-in-folder side button), mirroring the "New files" cards but with a
 *    neutral (non-green) accent. A removed file renders as a static
 *    destructive card (nothing to open). Collapsed by default; the grid
 *    scrolls within a bounded height.
 *
 * Every non-removed card carries an `InlineDiffToggle` that expands the file's
 * unified diff in place, below the grid. Each section is an accordion — at
 * most ONE file open at a time — and the diff is mounted only while expanded,
 * so a section that nobody expands never pays for a diff parse or its rows.
 * The "open in workspace diff tab" button stays for side-by-side reading.
 *
 * File changes are parsed lazily and ONLY once the reply is persisted
 * (`isResponseComplete`), so the streaming hot path never runs diff parsing.
 */
export const ReplyArtifacts = memo(function ReplyArtifacts({
  sourceTurns,
  isResponseComplete,
}: {
  sourceTurns: MessageTurn[]
  isResponseComplete: boolean
}) {
  const t = useTranslations("Folder.chat.replyArtifacts")
  const tCommon = useTranslations("Folder.common")
  const { activeFolder: folder } = useActiveFolder()
  const { openFilePreview, openSessionFileDiff } = useWorkspaceActions()
  const [newFilesOpen, setNewFilesOpen] = useState(true)
  const [changedOpen, setChangedOpen] = useState(false)
  // Accordion state: the id of the single file whose inline diff is open in
  // each section (`null` = none).
  const [expandedNewId, setExpandedNewId] = useState<string | null>(null)
  const [expandedChangedId, setExpandedChangedId] = useState<string | null>(
    null
  )

  // Guard parsing behind completion so mid-stream renders stay diff-free.
  const files = useMemo(
    () => (isResponseComplete ? extractReplyFileChanges(sourceTurns) : []),
    [isResponseComplete, sourceTurns]
  )

  // Split created files from modified/removed files — each lands in its own
  // section ("New files" vs "Files changed"). Removal wins over creation, so a
  // create+delete in the same reply lands in "changed", not "new files".
  const { addedFiles, changedFiles } = useMemo(() => {
    const addedFiles: FileChangeStat[] = []
    const changedFiles: FileChangeStat[] = []
    for (const file of files) {
      if (!isRemovedFileDiff(file.diff) && isAddedFileDiff(file.diff)) {
        addedFiles.push(file)
      } else {
        changedFiles.push(file)
      }
    }
    return { addedFiles, changedFiles }
  }, [files])

  if (!isResponseComplete) return null
  if (files.length === 0) return null

  const folderPath = folder?.path

  // Resolve the accordion ids against the CURRENT lists: an id left over from
  // an earlier parse simply resolves to nothing, so no stale panel can render.
  const expandedNew = addedFiles.find((f) => f.id === expandedNewId) ?? null
  const expandedChanged =
    changedFiles.find((f) => f.id === expandedChangedId) ?? null

  const openInTabs = (file: FileChangeStat) => {
    // openFilePreview accepts absolute paths (any location) and paths
    // relative to the active folder — agent-reported paths are one of the
    // two, so hand them over as-is.
    void openFilePreview(normalizeSlashPath(file.path))
  }

  const revealInFolder = (file: FileChangeStat) => {
    const absolute = toAbsoluteFilePath(file.path, folderPath)
    if (absolute) void revealItemInDir(absolute)
  }

  // Open the file's unified diff in an editor tab. Keyed by the reply's first
  // turn id so the same file changed by two different replies opens as two
  // distinct diff tabs instead of colliding into one. Works in web too (unlike
  // reveal), so it stays ungated by `isLocalDesktop()`.
  const replyDiffKey = sourceTurns[0]?.id ?? "reply"
  const viewDiff = (file: FileChangeStat) => {
    openSessionFileDiff(
      file.path,
      file.diff ?? t("noDiffDataAvailable", { filePath: file.path }),
      replyDiffKey
    )
  }

  const totalAdditions = changedFiles.reduce((sum, f) => sum + f.additions, 0)
  const totalDeletions = changedFiles.reduce((sum, f) => sum + f.deletions, 0)

  return (
    <div className="mt-2 space-y-2">
      {addedFiles.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-border bg-card/40 text-card-foreground">
          <button
            type="button"
            aria-expanded={newFilesOpen}
            onClick={() => setNewFilesOpen((prev) => !prev)}
            className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
          >
            <FilePlus className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="text-xs font-medium text-foreground">
              {t("newFilesTitle")}
            </span>
            <span className="rounded-md border border-border bg-muted/40 px-1.5 py-0.5 text-[10px] text-muted-foreground">
              {t("fileCount", { count: addedFiles.length })}
            </span>
            <ChevronRight
              className={cn(
                "ms-auto h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                newFilesOpen && "rotate-90"
              )}
            />
          </button>

          {newFilesOpen && (
            <TooltipProvider delayDuration={300}>
              <div className="border-t border-border">
                <div className="@container max-h-80 overflow-y-auto p-2">
                  <div className="grid gap-2 @md:grid-cols-2">
                    {addedFiles.map((file) => {
                      const displayPath = toFolderRelativePath(
                        file.path,
                        folderPath
                      )
                      const name = fileNameOf(displayPath)
                      const dir =
                        displayPath === name
                          ? ""
                          : displayPath.slice(
                              0,
                              displayPath.length - name.length - 1
                            )

                      return (
                        <div
                          key={file.id}
                          className="flex items-stretch overflow-hidden rounded-md border border-green-600/30 bg-green-500/5 transition-colors hover:border-green-600/50 hover:bg-green-500/10 dark:border-green-400/30 dark:hover:border-green-400/50"
                        >
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <button
                                type="button"
                                onClick={() => openInTabs(file)}
                                title={displayPath}
                                aria-label={t("openFile", {
                                  filePath: displayPath,
                                })}
                                className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 px-2.5 py-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                              >
                                <FileIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
                                <span className="flex min-w-0 flex-1 flex-col">
                                  <span className="truncate text-xs font-medium text-foreground">
                                    {name}
                                  </span>
                                  {dir && (
                                    <span className="truncate text-[10px] text-muted-foreground">
                                      {dir}
                                    </span>
                                  )}
                                </span>
                              </button>
                            </TooltipTrigger>
                            <TooltipContent side="top">
                              {t("openInEditor")}
                            </TooltipContent>
                          </Tooltip>

                          <InlineDiffToggle
                            file={file}
                            accent="green"
                            expanded={expandedNewId === file.id}
                            onToggle={() =>
                              setExpandedNewId((prev) =>
                                prev === file.id ? null : file.id
                              )
                            }
                          />

                          {isLocalDesktop() && (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <button
                                  type="button"
                                  onClick={() => revealInFolder(file)}
                                  aria-label={t("revealInFolder")}
                                  className="flex w-9 shrink-0 items-center justify-center border-l border-green-600/30 text-muted-foreground transition-colors hover:bg-green-500/15 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring dark:border-green-400/30"
                                >
                                  <ExternalLink className="h-3.5 w-3.5" />
                                </button>
                              </TooltipTrigger>
                              <TooltipContent side="top">
                                {t("revealInFolder")}
                              </TooltipContent>
                            </Tooltip>
                          )}
                        </div>
                      )
                    })}
                  </div>
                </div>

                {expandedNew?.diff ? (
                  <InlineDiffPanel
                    diff={expandedNew.diff}
                    displayPath={toFolderRelativePath(
                      expandedNew.path,
                      folderPath
                    )}
                  />
                ) : null}
              </div>
            </TooltipProvider>
          )}
        </div>
      )}

      {changedFiles.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-border bg-card/40 text-card-foreground">
          <button
            type="button"
            aria-expanded={changedOpen}
            onClick={() => setChangedOpen((prev) => !prev)}
            className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
          >
            <FileDiff className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="text-xs font-medium text-foreground">
              {t("title")}
            </span>
            <span className="rounded-md border border-border bg-muted/40 px-1.5 py-0.5 text-[10px] text-muted-foreground">
              {t("fileCount", { count: changedFiles.length })}
            </span>
            {/* Always render BOTH counts (incl. zeros) so a one-sided reply
                still shows its +N and -N. */}
            <span className="inline-flex items-center gap-1.5 rounded-md border border-border bg-muted/40 px-1.5 py-0.5 font-mono text-[10px]">
              <span className="text-green-600 dark:text-green-400">
                +{totalAdditions}
              </span>
              <span className="text-red-600 dark:text-red-400">
                -{totalDeletions}
              </span>
            </span>
            <ChevronRight
              className={cn(
                "ms-auto h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                changedOpen && "rotate-90"
              )}
            />
          </button>

          {changedOpen && (
            <TooltipProvider delayDuration={300}>
              <div className="border-t border-border">
                <div className="@container max-h-80 overflow-y-auto p-2">
                  <div className="grid gap-2 @md:grid-cols-2">
                    {changedFiles.map((file) => {
                      const displayPath = toFolderRelativePath(
                        file.path,
                        folderPath
                      )
                      const name = fileNameOf(displayPath)
                      const dir =
                        displayPath === name
                          ? ""
                          : displayPath.slice(
                              0,
                              displayPath.length - name.length - 1
                            )
                      const isRemoved = isRemovedFileDiff(file.diff)

                      // Removed files no longer exist on disk — there is
                      // nothing to open or reveal, so render a static
                      // (non-interactive) card that keeps the destructive
                      // accent and remove badge.
                      if (isRemoved) {
                        return (
                          <div
                            key={file.id}
                            title={displayPath}
                            className="flex items-center gap-2 overflow-hidden rounded-md border border-destructive/30 bg-destructive/5 px-2.5 py-2"
                          >
                            <FileIcon className="h-4 w-4 shrink-0 text-destructive" />
                            <span className="flex min-w-0 flex-1 flex-col">
                              <span className="truncate text-xs font-medium text-destructive">
                                {name}
                              </span>
                              {dir && (
                                <span className="truncate text-[10px] text-muted-foreground">
                                  {dir}
                                </span>
                              )}
                            </span>
                            <span className="inline-flex shrink-0 items-center rounded-md border border-destructive/30 bg-destructive/10 px-1.5 py-0.5 font-mono text-[10px] text-destructive">
                              {t("remove")}
                            </span>
                          </div>
                        )
                      }

                      return (
                        <div
                          key={file.id}
                          className="flex items-stretch overflow-hidden rounded-md border border-border bg-muted/20 transition-colors hover:bg-accent/40"
                        >
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <button
                                type="button"
                                onClick={() => openInTabs(file)}
                                title={displayPath}
                                aria-label={t("openFile", {
                                  filePath: displayPath,
                                })}
                                className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 px-2.5 py-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                              >
                                <FileIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
                                <span className="flex min-w-0 flex-1 flex-col">
                                  <span className="truncate text-xs font-medium text-foreground">
                                    {name}
                                  </span>
                                  {dir && (
                                    <span className="truncate text-[10px] text-muted-foreground">
                                      {dir}
                                    </span>
                                  )}
                                </span>
                              </button>
                            </TooltipTrigger>
                            <TooltipContent side="top">
                              {t("openInEditor")}
                            </TooltipContent>
                          </Tooltip>

                          <InlineDiffToggle
                            file={file}
                            accent="neutral"
                            expanded={expandedChangedId === file.id}
                            onToggle={() =>
                              setExpandedChangedId((prev) =>
                                prev === file.id ? null : file.id
                              )
                            }
                          />

                          <Tooltip>
                            <TooltipTrigger asChild>
                              <button
                                type="button"
                                onClick={() => viewDiff(file)}
                                aria-label={tCommon("viewDiff")}
                                className="flex w-9 shrink-0 items-center justify-center border-l border-border text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                              >
                                <FileDiff className="h-3.5 w-3.5" />
                              </button>
                            </TooltipTrigger>
                            <TooltipContent side="top">
                              {tCommon("viewDiff")}
                            </TooltipContent>
                          </Tooltip>

                          {isLocalDesktop() && (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <button
                                  type="button"
                                  onClick={() => revealInFolder(file)}
                                  aria-label={t("revealInFolder")}
                                  className="flex w-9 shrink-0 items-center justify-center border-l border-border text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                                >
                                  <ExternalLink className="h-3.5 w-3.5" />
                                </button>
                              </TooltipTrigger>
                              <TooltipContent side="top">
                                {t("revealInFolder")}
                              </TooltipContent>
                            </Tooltip>
                          )}
                        </div>
                      )
                    })}
                  </div>
                </div>

                {expandedChanged?.diff ? (
                  <InlineDiffPanel
                    diff={expandedChanged.diff}
                    displayPath={toFolderRelativePath(
                      expandedChanged.path,
                      folderPath
                    )}
                  />
                ) : null}
              </div>
            </TooltipProvider>
          )}
        </div>
      )}
    </div>
  )
})

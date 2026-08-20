"use client"

import { useState, useEffect, useRef, useCallback, useMemo } from "react"
import { formatDistanceToNow } from "date-fns"
import { enUS, zhCN, zhTW } from "date-fns/locale"
import { File, Folder, LibraryBig } from "lucide-react"
import { useLocale, useTranslations } from "next-intl"
import { useAuxPanelContext } from "@/contexts/aux-panel-context"
import { useActiveFolder } from "@/contexts/active-folder-context"
import { useSessionCenter } from "@/contexts/session-center-context"
import { useOpenOrFocusSession } from "@/hooks/use-open-or-focus-session"
import { useWorkspaceActions } from "@/contexts/workspace-context"
import { listAllConversations } from "@/lib/api"
import type { ConversationStatus, DbConversationSummary } from "@/lib/types"
import { useFileTree, type FlatFileEntry } from "@/hooks/use-file-tree"
import { rankFileMatches } from "@/lib/file-search-match"
import { getAgentLabel } from "@/lib/custom-agents"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import {
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
} from "@/components/ui/command"
import { cn } from "@/lib/utils"
import { formatConversationTitle } from "@/lib/conversation-title"

type SearchTab = "conversations" | "files"
type ConversationScope = "all" | "folder"

interface SearchCommandDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * The command palette's conversation tab is deliberately shallow: type a few
 * characters, jump to a Session by title. Content search and the narrowing
 * facets belong to the Session Center, which owns that job with a scope switch
 * and a full filter set — so the palette hands the query over instead of
 * reimplementing a second, weaker copy of it.
 */
export function SearchCommandDialog({
  open,
  onOpenChange,
}: SearchCommandDialogProps) {
  const t = useTranslations("Folder.search")
  const locale = useLocale()
  const dateFnsLocale =
    locale === "zh-CN" ? zhCN : locale === "zh-TW" ? zhTW : enUS
  const { activeFolder: folder, activeFolderId } = useActiveFolder()
  const folderId = activeFolderId ?? 0
  const openOrFocusSession = useOpenOrFocusSession()
  const { openFilePreview } = useWorkspaceActions()
  const { revealInFileTree } = useAuxPanelContext()
  const { openSessionCenter } = useSessionCenter()

  const [activeTab, setActiveTab] = useState<SearchTab>("conversations")
  const [conversationScope, setConversationScope] =
    useState<ConversationScope>("all")
  const [query, setQuery] = useState("")
  const [results, setResults] = useState<DbConversationSummary[]>([])
  const [searching, setSearching] = useState(false)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined)
  const requestEpochRef = useRef(0)

  const folderPath = folder?.path ?? ""

  // File search via shared hook (lazy-loaded when files tab is active)
  const {
    allFiles,
    loading: filesLoading,
    reset: resetFileTree,
  } = useFileTree({
    folderPath: folderPath || undefined,
    enabled: activeTab === "files",
  })

  // Rank files by relevance (name/path tiers + fuzzy subsequence), scanning the
  // full list so a deeply nested match isn't crowded out by shallower ones.
  const filteredFiles = useMemo(
    () => rankFileMatches(query, allFiles, 100),
    [allFiles, query]
  )

  const doSearch = useCallback(
    async (q: string, scope: ConversationScope) => {
      const epoch = ++requestEpochRef.current
      const normalized = q.trim()
      if (!normalized) {
        setResults([])
        setSearching(false)
        return
      }
      setSearching(true)
      const folderIds = scope === "folder" && folderId > 0 ? [folderId] : null
      try {
        const rows = await listAllConversations({
          folder_ids: folderIds,
          search: normalized,
        })
        if (epoch !== requestEpochRef.current) return
        setResults(rows)
      } catch {
        if (epoch !== requestEpochRef.current) return
        setResults([])
      } finally {
        if (epoch === requestEpochRef.current) setSearching(false)
      }
    },
    [folderId]
  )

  // Debounced search on query change (conversations tab only)
  useEffect(() => {
    if (activeTab !== "conversations") return
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      doSearch(query, conversationScope)
    }, 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [query, conversationScope, doSearch, activeTab])

  // Reset state when dialog closes
  useEffect(() => {
    if (!open) {
      setQuery("")
      setResults([])
      setActiveTab("conversations")
      setConversationScope("all")
      requestEpochRef.current += 1
      resetFileTree()
    }
  }, [open, resetFileTree])

  const handleSelectConversation = useCallback(
    (conv: DbConversationSummary) => {
      void openOrFocusSession(conv)
      onOpenChange(false)
    },
    [openOrFocusSession, onOpenChange]
  )

  const handleOpenSessionCenter = useCallback(() => {
    onOpenChange(false)
    openSessionCenter({ search: query.trim() })
  }, [onOpenChange, openSessionCenter, query])

  const handleSelectFile = useCallback(
    (entry: FlatFileEntry) => {
      if (entry.kind === "dir") {
        revealInFileTree(entry.relativePath)
      } else {
        // Reveal parent directory in file tree, then open the file
        const lastSlash = entry.relativePath.lastIndexOf("/")
        if (lastSlash > 0) {
          revealInFileTree(entry.relativePath.slice(0, lastSlash))
        }
        openFilePreview(entry.relativePath)
      }
      onOpenChange(false)
    },
    [revealInFileTree, openFilePreview, onOpenChange]
  )

  // Neither tab does what its name suggests at first glance — files matches
  // names and paths, never file contents; conversations matches titles, never
  // message text. Both say so in their own placeholder + empty state rather
  // than leaving the user to discover it by getting no hits.
  const filePlaceholder = folder
    ? t("filePlaceholderInFolder", { name: folder.name })
    : t("filePlaceholder")
  const filesScopeHint = folder
    ? t("filesScopeHint", { name: folder.name })
    : t("filesScopeHintNoFolder")
  const placeholder =
    activeTab === "conversations" ? t("placeholder") : filePlaceholder
  const trimmedQuery = query.trim()

  return (
    <CommandDialog
      title={
        folder && conversationScope === "folder"
          ? t("dialogTitleWithFolder", { name: folder.name })
          : t("dialogTitle")
      }
      open={open}
      onOpenChange={onOpenChange}
      shouldFilter={activeTab === "conversations"}
    >
      {/* Search scope: global Session Library by default, with a one-click
          current-folder view for users working inside a single project. */}
      {activeTab === "conversations" && (
        <div className="flex items-center gap-2 border-b px-4 py-2.5">
          <Folder className="w-4 h-4 shrink-0 text-muted-foreground" />
          <button
            onClick={() => setConversationScope("all")}
            className={cn(
              "h-6 rounded-md px-2 text-xs transition-colors",
              conversationScope === "all"
                ? "bg-secondary text-secondary-foreground"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            {t("scopeAll")}
          </button>
          {folder && (
            <button
              onClick={() => setConversationScope("folder")}
              className={cn(
                "h-6 min-w-0 truncate rounded-md px-2 text-xs transition-colors",
                conversationScope === "folder"
                  ? "bg-secondary text-secondary-foreground"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              {t("scopeFolder", { name: folder.name })}
            </button>
          )}
        </div>
      )}

      {/* Tabs */}
      <div className="flex items-center gap-0 border-b px-3">
        <button
          onClick={() => setActiveTab("conversations")}
          className={cn(
            "relative h-9 px-3 text-sm font-medium transition-colors",
            activeTab === "conversations"
              ? "text-foreground"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          {t("tabConversations")}
          {activeTab === "conversations" && (
            <span className="absolute bottom-0 left-3 right-3 h-0.5 bg-foreground rounded-full" />
          )}
        </button>
        <button
          onClick={() => setActiveTab("files")}
          className={cn(
            "relative h-9 px-3 text-sm font-medium transition-colors",
            activeTab === "files"
              ? "text-foreground"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          {t("tabFiles")}
          {activeTab === "files" && (
            <span className="absolute bottom-0 left-3 right-3 h-0.5 bg-foreground rounded-full" />
          )}
        </button>
      </div>

      <CommandInput
        placeholder={placeholder}
        value={query}
        onValueChange={setQuery}
      />

      <CommandList className="min-h-96">
        {/* Conversations tab */}
        {activeTab === "conversations" && (
          <>
            <CommandEmpty>
              {searching ? (
                t("searching")
              ) : (
                <span className="flex flex-col items-center gap-1">
                  <span>
                    {trimmedQuery ? t("noResults") : t("typeToSearch")}
                  </span>
                  <span className="text-xs text-muted-foreground/80">
                    {t("conversationsScopeHint")}
                  </span>
                </span>
              )}
            </CommandEmpty>
            {results.length > 0 && (
              <CommandGroup>
                {results.map((conv) => (
                  <CommandItem
                    key={conv.id}
                    value={`${conv.id}-${formatConversationTitle(conv.title)}`}
                    onSelect={() => handleSelectConversation(conv)}
                  >
                    <ConversationStatusDot
                      status={conv.status as ConversationStatus}
                    />
                    <span className="flex-1 truncate">
                      {formatConversationTitle(conv.title) ||
                        t("untitledConversation")}
                    </span>
                    <span className="text-xs text-muted-foreground shrink-0">
                      {getAgentLabel(conv.agent_type)}
                    </span>
                    <span className="text-xs text-muted-foreground shrink-0">
                      {formatDistanceToNow(new Date(conv.created_at), {
                        addSuffix: true,
                        locale: dateFnsLocale,
                      })}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </>
        )}

        {/* Files tab */}
        {activeTab === "files" && (
          <>
            <CommandEmpty>
              {filesLoading ? (
                t("searching")
              ) : (
                <span className="flex flex-col items-center gap-1">
                  <span>
                    {trimmedQuery ? t("noResults") : t("typeToSearchFiles")}
                  </span>
                  <span className="text-xs text-muted-foreground/80">
                    {filesScopeHint}
                  </span>
                </span>
              )}
            </CommandEmpty>
            {filteredFiles.length > 0 && (
              <CommandGroup>
                {filteredFiles.map((entry) => (
                  <CommandItem
                    key={entry.relativePath}
                    value={entry.relativePath}
                    onSelect={() => handleSelectFile(entry)}
                  >
                    {entry.kind === "dir" ? (
                      <Folder className="w-4 h-4 shrink-0 text-blue-500" />
                    ) : (
                      <File className="w-4 h-4 shrink-0 text-muted-foreground" />
                    )}
                    <span className="flex-1 truncate">{entry.name}</span>
                    <span className="text-xs text-muted-foreground shrink-0 truncate max-w-48">
                      {entry.relativePath}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </>
        )}
      </CommandList>

      {/* Handover to the Session Center, pinned below the list rather than
          rendered as a CommandItem: cmdk filters items against the query, and
          this row has to stay reachable exactly when nothing matched. */}
      {activeTab === "conversations" && (
        <button
          type="button"
          onClick={handleOpenSessionCenter}
          className="flex w-full items-center gap-2 border-t px-4 py-2.5 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
        >
          <LibraryBig className="h-4 w-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate text-start">
            {trimmedQuery
              ? t("searchInSessionCenter", { query: trimmedQuery })
              : t("openSessionCenter")}
          </span>
        </button>
      )}
    </CommandDialog>
  )
}

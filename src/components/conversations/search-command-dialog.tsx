"use client"

import { useState, useEffect, useRef, useCallback, useMemo } from "react"
import { formatDistanceToNow } from "date-fns"
import { enUS, zhCN, zhTW } from "date-fns/locale"
import { File, Folder, MessageSquareText } from "lucide-react"
import { useLocale, useTranslations } from "next-intl"
import { useAuxPanelContext } from "@/contexts/aux-panel-context"
import { useActiveFolder } from "@/contexts/active-folder-context"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useTabActions } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { useWorkspaceActions } from "@/contexts/workspace-context"
import { listAllConversations, searchSessionContent } from "@/lib/api"
import type {
  AgentType,
  ConversationStatus,
  DbConversationSummary,
  SessionContentSearchResponse,
} from "@/lib/types"
import { useFileTree, type FlatFileEntry } from "@/hooks/use-file-tree"
import { rankFileMatches } from "@/lib/file-search-match"
import { compareAgentType } from "@/lib/types"
import { getAgentLabel } from "@/lib/custom-agents"
import { AgentIcon } from "@/components/agent-icon"
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

export function SearchCommandDialog({
  open,
  onOpenChange,
}: SearchCommandDialogProps) {
  const t = useTranslations("Folder.search")
  const locale = useLocale()
  const dateFnsLocale =
    locale === "zh-CN" ? zhCN : locale === "zh-TW" ? zhTW : enUS
  const { activeFolder: folder, activeFolderId } = useActiveFolder()
  const allConversations = useAppWorkspaceStore((s) => s.conversations)
  const allFolders = useAppWorkspaceStore((s) => s.allFolders)
  const folderId = activeFolderId ?? 0
  const conversations = useMemo(
    () =>
      activeFolderId == null
        ? []
        : allConversations.filter((c) => c.folder_id === activeFolderId),
    [allConversations, activeFolderId]
  )
  const { openTab } = useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const { openFilePreview } = useWorkspaceActions()
  const { revealInFileTree } = useAuxPanelContext()

  const [activeTab, setActiveTab] = useState<SearchTab>("conversations")
  const [conversationScope, setConversationScope] =
    useState<ConversationScope>("all")
  const [query, setQuery] = useState("")
  const [agentFilter, setAgentFilter] = useState<AgentType | null>(null)
  const [results, setResults] = useState<DbConversationSummary[]>([])
  const [contentSearch, setContentSearch] =
    useState<SessionContentSearchResponse | null>(null)
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

  const scopedConversations =
    conversationScope === "folder" && activeFolderId != null
      ? conversations
      : allConversations

  // Compute which agent types exist in the selected conversation scope.
  const availableAgents = Array.from(
    new Set(scopedConversations.map((c) => c.agent_type))
  ).sort(compareAgentType)
  const folderNames = useMemo(
    () => new Map(allFolders.map((item) => [item.id, item.name])),
    [allFolders]
  )

  // Rank files by relevance (name/path tiers + fuzzy subsequence), scanning the
  // full list so a deeply nested match isn't crowded out by shallower ones.
  const filteredFiles = useMemo(
    () => rankFileMatches(query, allFiles, 100),
    [allFiles, query]
  )

  const doSearch = useCallback(
    async (q: string, agent: AgentType | null, scope: ConversationScope) => {
      const epoch = ++requestEpochRef.current
      if (!q.trim() && !agent) {
        setResults([])
        setContentSearch(null)
        setSearching(false)
        return
      }
      setSearching(true)
      const folderIds = scope === "folder" && folderId > 0 ? [folderId] : null
      try {
        const normalized = q.trim()
        const [metadataResult, contentResult] = await Promise.allSettled([
          listAllConversations({
            folder_ids: folderIds,
            search: normalized || null,
            agent_type: agent,
          }),
          normalized.length >= 2
            ? searchSessionContent({
                query: normalized,
                folder_ids: folderIds,
                agent_type: agent,
                limit: 30,
              })
            : Promise.resolve(null),
        ])
        if (epoch !== requestEpochRef.current) return
        setResults(
          metadataResult.status === "fulfilled" ? metadataResult.value : []
        )
        setContentSearch(
          contentResult.status === "fulfilled" ? contentResult.value : null
        )
      } catch {
        if (epoch !== requestEpochRef.current) return
        setResults([])
        setContentSearch(null)
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
      doSearch(query, agentFilter, conversationScope)
    }, 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [query, agentFilter, conversationScope, doSearch, activeTab])

  // Reset state when dialog closes
  useEffect(() => {
    if (!open) {
      setQuery("")
      setAgentFilter(null)
      setResults([])
      setContentSearch(null)
      setActiveTab("conversations")
      setConversationScope("all")
      requestEpochRef.current += 1
      resetFileTree()
    }
  }, [open, resetFileTree])

  const handleSelectConversation = useCallback(
    (conv: DbConversationSummary) => {
      // Leave any workbench route (e.g. Automations) so the picked conversation
      // isn't stranded behind the route overlay — covers re-selecting the
      // already-active tab, which doesn't change activeTabId.
      openConversations()
      openTab(conv.folder_id, conv.id, conv.agent_type, true)
      onOpenChange(false)
    },
    [openTab, onOpenChange, openConversations]
  )

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

  const placeholder =
    activeTab === "conversations" ? t("placeholder") : t("filePlaceholder")

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

      {/* Agent filter (conversations tab only). Wraps: one chip per agent type
          present in the folder, each carrying a full name, so a workspace with
          a dozen enabled agents runs past the dialog — which is
          `overflow-hidden`, so the tail chips were clipped away and simply
          could not be clicked. Wrapping keeps every filter reachable and lets
          the block grow by a row instead of hiding options. */}
      {activeTab === "conversations" && availableAgents.length > 1 && (
        <div className="flex flex-wrap items-center gap-1 px-3 py-2 border-b">
          <button
            onClick={() => setAgentFilter(null)}
            className={cn(
              "h-6 shrink-0 text-xs px-2 rounded-md transition-colors",
              agentFilter === null
                ? "bg-secondary text-secondary-foreground"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            {t("allAgents")}
          </button>
          {availableAgents.map((at) => (
            <button
              key={at}
              onClick={() => setAgentFilter(at)}
              className={cn(
                "flex shrink-0 items-center gap-1.5 h-6 text-xs px-2 rounded-md transition-colors",
                agentFilter === at
                  ? "bg-secondary text-secondary-foreground"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              <AgentIcon agentType={at} className="w-3.5 h-3.5" />
              {getAgentLabel(at)}
            </button>
          ))}
        </div>
      )}

      <CommandList className="min-h-96">
        {/* Conversations tab */}
        {activeTab === "conversations" && (
          <>
            <CommandEmpty>
              {searching
                ? t("searching")
                : !query.trim() && !agentFilter
                  ? t("typeToSearch")
                  : t("noResults")}
            </CommandEmpty>
            {results.length > 0 && (
              <CommandGroup heading={t("metadataMatches")}>
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
            {contentSearch && !contentSearch.available && query.trim() && (
              <div className="border-t px-4 py-2 text-xs text-muted-foreground">
                {t("contentUnavailable")}
              </div>
            )}
            {contentSearch?.available && contentSearch.results.length > 0 && (
              <CommandGroup heading={t("contentMatches")}>
                {contentSearch.results.map((hit) => {
                  const conv = hit.conversation
                  const matchedAt = hit.matched_at
                    ? formatDistanceToNow(new Date(hit.matched_at), {
                        addSuffix: true,
                        locale: dateFnsLocale,
                      })
                    : null
                  return (
                    <CommandItem
                      key={`content-${conv.id}`}
                      value={`${conv.id}-${formatConversationTitle(conv.title)}-${hit.snippet}`}
                      onSelect={() => handleSelectConversation(conv)}
                      className="items-start py-2"
                    >
                      <MessageSquareText className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-2">
                          <span className="truncate font-medium">
                            {formatConversationTitle(conv.title) ||
                              t("untitledConversation")}
                          </span>
                          <span className="shrink-0 text-xs text-muted-foreground">
                            {getAgentLabel(conv.agent_type)}
                          </span>
                        </span>
                        <span className="line-clamp-2 whitespace-pre-line text-xs text-muted-foreground">
                          {hit.snippet}
                        </span>
                        <span className="flex gap-2 text-[0.6875rem] text-muted-foreground/80">
                          <span>{folderNames.get(conv.folder_id) ?? ""}</span>
                          {matchedAt && <span>{matchedAt}</span>}
                          {hit.more_matches > 0 && (
                            <span>
                              {t("moreContentMatches", {
                                count: hit.more_matches,
                              })}
                            </span>
                          )}
                        </span>
                      </span>
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            )}
          </>
        )}

        {/* Files tab */}
        {activeTab === "files" && (
          <>
            <CommandEmpty>
              {filesLoading
                ? t("searching")
                : !query.trim()
                  ? t("typeToSearchFiles")
                  : t("noResults")}
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
    </CommandDialog>
  )
}

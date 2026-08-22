"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import {
  deletePromptQueueItem,
  editPromptQueueItem,
  enqueuePromptQueueItem,
  getPromptQueue,
  reorderPromptQueueItems,
  pausePromptQueueForManualReview,
  releaseOnePromptQueueItem,
  resumePromptQueue,
  retryPromptQueueItem,
} from "@/lib/api"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import {
  PROMPT_QUEUE_CHANGED_EVENT,
  type PromptDraft,
  type PromptQueueItem,
  type PromptQueueSource,
  type PromptQueueSnapshot,
} from "@/lib/types"
import { randomUUID } from "@/lib/utils"

export interface QueuedMessage {
  id: string
  draft: PromptDraft | null
  taskId: number | null
  modeId: string | null
  state: "queued" | "claimed" | "paused"
  source: PromptQueueSource
  attempts: number
  pausedReason: string | null
}

export interface UseMessageQueueReturn {
  queue: QueuedMessage[]
  revision: number
  pausedReason: string | null
  manualReleaseItemId: string | null
  hydrated: boolean
  enqueue: (draft: PromptDraft, modeId: string | null) => void
  remove: (id: string) => void
  reorder: (items: QueuedMessage[]) => void
  updateItem: (id: string, draft: PromptDraft) => void
  retryItem: (id: string) => void
  pauseManual: () => void
  releaseOne: (id: string) => void
  resume: () => void
  getQueueLength: () => number
  editingItemId: string | null
  startEditing: (id: string) => void
  cancelEditing: () => void
}

interface UseMessageQueueOptions {
  onPersistFailure?: (draft: PromptDraft, error: unknown) => void
}

function fromWire(item: PromptQueueItem): QueuedMessage {
  return {
    id: item.id,
    draft: item.draft ?? null,
    taskId: item.taskId ?? null,
    modeId: item.modeId ?? null,
    state: item.state,
    // A dev-mode HMR frontend can outlive the backend it talks to; snapshots
    // from a pre-`source` backend simply lack the field, and "user" is the
    // only class such a backend ever surfaced here (drafts render, letters
    // and reminders queue draftless).
    source: item.source ?? "user",
    attempts: item.attempts,
    pausedReason: item.pausedReason ?? null,
  }
}

export function useMessageQueue(
  conversationId: number | null,
  options: UseMessageQueueOptions = {}
): UseMessageQueueReturn {
  const [queue, setQueue] = useState<QueuedMessage[]>([])
  const [revision, setRevision] = useState(0)
  const [pausedReason, setPausedReason] = useState<string | null>(null)
  const [manualReleaseItemId, setManualReleaseItemId] = useState<string | null>(
    null
  )
  const [hydrated, setHydrated] = useState(conversationId == null)
  const [editingItemId, setEditingItemId] = useState<string | null>(null)
  const queueRef = useRef<QueuedMessage[]>([])
  const revisionRef = useRef(0)
  const conversationIdRef = useRef(conversationId)
  const loadedConversationIdRef = useRef(conversationId)
  const pendingOptimisticRef = useRef(new Map<string, QueuedMessage>())
  const generationRef = useRef(0)
  const persistFailureRef = useRef(options.onPersistFailure)

  useEffect(() => {
    persistFailureRef.current = options.onPersistFailure
  }, [options.onPersistFailure])

  useEffect(() => {
    conversationIdRef.current = conversationId
  }, [conversationId])

  const commit = useCallback((next: QueuedMessage[]) => {
    queueRef.current = next
    setQueue(next)
  }, [])

  const applySnapshot = useCallback(
    (snapshot: PromptQueueSnapshot) => {
      if (snapshot.conversationId !== conversationIdRef.current) return
      if (snapshot.revision < revisionRef.current) return
      revisionRef.current = snapshot.revision
      setRevision(snapshot.revision)
      setPausedReason(snapshot.pausedReason ?? null)
      setManualReleaseItemId(snapshot.manualReleaseItemId ?? null)
      const backend = snapshot.items.map(fromWire)
      const backendIds = new Set(backend.map((item) => item.id))
      // Keep a just-clicked optimistic append visible until its own request
      // resolves. An older fetch/event cannot erase it; once the request
      // returns we remove it from this map and the authoritative snapshot wins.
      for (const item of pendingOptimisticRef.current.values()) {
        if (!backendIds.has(item.id)) backend.push(item)
      }
      commit(backend)
      setEditingItemId((current) =>
        current && backend.some((item) => item.id === current) ? current : null
      )
      setHydrated(true)
    },
    [commit]
  )

  const reload = useCallback(async () => {
    const id = conversationIdRef.current
    if (id == null) return
    const generation = generationRef.current
    try {
      const snapshot = await getPromptQueue(id)
      if (generation === generationRef.current) applySnapshot(snapshot)
    } catch (error) {
      console.error("[prompt-queue] snapshot:", error)
      if (generation === generationRef.current) setHydrated(true)
    }
  }, [applySnapshot])

  useEffect(() => {
    const previousConversationId = loadedConversationIdRef.current
    loadedConversationIdRef.current = conversationId
    generationRef.current += 1
    const generation = generationRef.current
    revisionRef.current = 0
    // Session identity changed, so stale queue state must disappear in the
    // same commit before the new external subscription can publish data.
    /* eslint-disable react-hooks/set-state-in-effect -- intentional identity-bound subscription reset */
    setRevision(0)
    setPausedReason(null)
    setManualReleaseItemId(null)
    setEditingItemId(null)
    // Preserve only the intentional draft -> persisted Session promotion. A
    // real Session switch must never flash or accidentally promote the old
    // Session's optimistic queue into the new one.
    if (!(previousConversationId == null && conversationId != null)) {
      pendingOptimisticRef.current.clear()
      commit([])
    }
    if (conversationId == null) {
      setHydrated(true)
      return
    }
    setHydrated(false)
    /* eslint-enable react-hooks/set-state-in-effect */
    void reload()

    let disposed = false
    let unsubscribe: (() => void) | undefined
    void subscribe<PromptQueueSnapshot>(
      PROMPT_QUEUE_CHANGED_EVENT,
      (snapshot) => {
        if (!disposed && generation === generationRef.current) {
          applySnapshot(snapshot)
        }
      }
    ).then((off) => {
      if (disposed) off()
      else unsubscribe = off
    })
    const offReconnect = onTransportReconnect(() => {
      if (!disposed) void reload()
    })
    return () => {
      disposed = true
      unsubscribe?.()
      offReconnect?.()
    }
  }, [conversationId, applySnapshot, commit, reload])

  // A prompt can be queued in the narrow window after a new Session's first
  // send starts but before its DB id reaches this hook. Preserve it locally,
  // then promote it with the SAME stable id as soon as the id arrives.
  useEffect(() => {
    if (conversationId == null) return
    for (const item of pendingOptimisticRef.current.values()) {
      void enqueuePromptQueueItem({
        conversationId,
        id: item.id,
        clientDedupeId: item.id,
        draft: item.draft!,
        modeId: item.modeId,
      })
        .then((snapshot) => {
          pendingOptimisticRef.current.delete(item.id)
          applySnapshot(snapshot)
        })
        .catch((error) => {
          pendingOptimisticRef.current.delete(item.id)
          console.error("[prompt-queue] promote local item:", error)
          persistFailureRef.current?.(item.draft!, error)
          void reload()
        })
    }
  }, [conversationId, applySnapshot, reload])

  const enqueue = useCallback(
    (draft: PromptDraft, modeId: string | null) => {
      const item: QueuedMessage = {
        id: randomUUID(),
        draft,
        taskId: null,
        modeId,
        state: "queued",
        source: "user",
        attempts: 0,
        pausedReason: null,
      }
      pendingOptimisticRef.current.set(item.id, item)
      commit([...queueRef.current, item])
      const id = conversationIdRef.current
      if (id == null) return
      void enqueuePromptQueueItem({
        conversationId: id,
        id: item.id,
        clientDedupeId: item.id,
        draft,
        modeId,
      })
        .then((snapshot) => {
          pendingOptimisticRef.current.delete(item.id)
          applySnapshot(snapshot)
        })
        .catch((error) => {
          pendingOptimisticRef.current.delete(item.id)
          console.error("[prompt-queue] enqueue:", error)
          persistFailureRef.current?.(draft, error)
          void reload()
        })
    },
    [applySnapshot, commit, reload]
  )

  const remove = useCallback(
    (id: string) => {
      if (editingItemId === id) setEditingItemId(null)
      pendingOptimisticRef.current.delete(id)
      commit(queueRef.current.filter((item) => item.id !== id))
      const activeConversationId = conversationIdRef.current
      if (activeConversationId == null) return
      void deletePromptQueueItem(activeConversationId, id, revisionRef.current)
        .then(applySnapshot)
        .catch((error) => {
          console.error("[prompt-queue] delete:", error)
          void reload()
        })
    },
    [applySnapshot, commit, editingItemId, reload]
  )

  const reorder = useCallback(
    (items: QueuedMessage[]) => {
      const current = queueRef.current
      if (items.length !== current.length) return
      const byId = new Map(current.map((item) => [item.id, item]))
      const seen = new Set<string>()
      const next: QueuedMessage[] = []
      for (const item of items) {
        const authoritative = byId.get(item.id)
        if (!authoritative || seen.has(item.id)) return
        seen.add(item.id)
        next.push(authoritative)
      }
      commit(next)
      const activeConversationId = conversationIdRef.current
      if (activeConversationId == null) return
      void reorderPromptQueueItems(
        activeConversationId,
        next.filter((item) => item.draft != null).map((item) => item.id),
        revisionRef.current
      )
        .then(applySnapshot)
        .catch((error) => {
          console.error("[prompt-queue] reorder:", error)
          void reload()
        })
    },
    [applySnapshot, commit, reload]
  )

  const updateItem = useCallback(
    (id: string, draft: PromptDraft) => {
      commit(
        queueRef.current.map((item) =>
          item.id === id ? { ...item, draft } : item
        )
      )
      setEditingItemId(null)
      const activeConversationId = conversationIdRef.current
      if (activeConversationId == null) {
        const pending = pendingOptimisticRef.current.get(id)
        if (pending) pendingOptimisticRef.current.set(id, { ...pending, draft })
        return
      }
      void editPromptQueueItem(
        activeConversationId,
        id,
        draft,
        revisionRef.current
      )
        .then(applySnapshot)
        .catch((error) => {
          console.error("[prompt-queue] edit:", error)
          void reload()
        })
    },
    [applySnapshot, commit, reload]
  )

  const retryItem = useCallback(
    (id: string) => {
      const activeConversationId = conversationIdRef.current
      if (activeConversationId == null) return
      void retryPromptQueueItem(activeConversationId, id, revisionRef.current)
        .then(applySnapshot)
        .catch((error) => {
          console.error("[prompt-queue] retry:", error)
          void reload()
        })
    },
    [applySnapshot, reload]
  )

  const resume = useCallback(() => {
    const activeConversationId = conversationIdRef.current
    if (activeConversationId == null) return
    void resumePromptQueue(activeConversationId, revisionRef.current)
      .then(applySnapshot)
      .catch((error) => {
        console.error("[prompt-queue] resume:", error)
        void reload()
      })
  }, [applySnapshot, reload])

  const pauseManual = useCallback(() => {
    const activeConversationId = conversationIdRef.current
    if (activeConversationId == null) return
    void pausePromptQueueForManualReview(
      activeConversationId,
      revisionRef.current
    )
      .then(applySnapshot)
      .catch((error) => {
        console.error("[prompt-queue] pause manual:", error)
        void reload()
      })
  }, [applySnapshot, reload])

  const releaseOne = useCallback(
    (id: string) => {
      const activeConversationId = conversationIdRef.current
      if (activeConversationId == null) return
      void releaseOnePromptQueueItem(
        activeConversationId,
        id,
        revisionRef.current
      )
        .then(applySnapshot)
        .catch((error) => {
          console.error("[prompt-queue] release one:", error)
          void reload()
        })
    },
    [applySnapshot, reload]
  )

  const getQueueLength = useCallback(() => queueRef.current.length, [])
  const startEditing = useCallback((id: string) => setEditingItemId(id), [])
  const cancelEditing = useCallback(() => setEditingItemId(null), [])

  return {
    queue,
    revision,
    pausedReason,
    manualReleaseItemId,
    hydrated,
    enqueue,
    remove,
    reorder,
    updateItem,
    retryItem,
    pauseManual,
    releaseOne,
    resume,
    getQueueLength,
    editingItemId,
    startEditing,
    cancelEditing,
  }
}

import type { Editor } from "@tiptap/core"

import type { ReferenceAttrs } from "@/components/chat/composer/types"
import type { PromptInputBlock } from "@/lib/types"

const SESSION_URI = /codeg:\/\/session\/(\d+)/gi

function addSessionId(
  ids: Set<number>,
  raw: string | number | null | undefined,
  excludeId?: number | null
) {
  const id = typeof raw === "number" ? raw : Number(raw)
  if (!Number.isInteger(id) || id <= 0 || id === excludeId) return
  ids.add(id)
}

/** Stable numeric Session ids mentioned as `codeg://session/<id>` in prose. */
export function sessionIdsFromText(
  text: string,
  excludeId?: number | null
): number[] {
  const ids = new Set<number>()
  for (const match of text.matchAll(SESSION_URI)) {
    addSessionId(ids, match[1], excludeId)
  }
  return [...ids]
}

const HUMAN_URI = /codeg:\/\/(?:human|user)(?![a-z0-9])/i

/** Structured `@human` / `@user` chips. Free-text `@user` is ignored. */
export function mentionsHumanFromText(text: string): boolean {
  return HUMAN_URI.test(text)
}

export function sessionIdsFromPromptBlocks(
  blocks: PromptInputBlock[],
  excludeId?: number | null
): number[] {
  const ids = new Set<number>()
  for (const block of blocks) {
    if (block.type !== "text" || typeof block.text !== "string") continue
    for (const id of sessionIdsFromText(block.text, excludeId)) ids.add(id)
  }
  return [...ids]
}

/** Structured `@` Session badges currently in the composer document. */
export function sessionIdsFromEditor(
  editor: Editor | null | undefined,
  excludeId?: number | null
): number[] {
  if (!editor) return []
  const ids = new Set<number>()
  editor.state.doc.descendants((node) => {
    if (node.type.name !== "reference") return
    const attrs = node.attrs as ReferenceAttrs
    if (attrs.refType !== "session") return
    const fromUri = attrs.uri?.match(/codeg:\/\/session\/(\d+)$/i)
    addSessionId(ids, fromUri?.[1] ?? attrs.id, excludeId)
  })
  return [...ids]
}

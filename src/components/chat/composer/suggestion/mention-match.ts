import {
  findSuggestionMatch,
  type SuggestionMatch,
  type Trigger,
} from "@tiptap/suggestion"

const ALLOWED_MENTION_PREFIX =
  /^[\s\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\p{Script=Bopomofo}　-〿︰-﹏＀-￯·゠・ー]$/u

/** Allow natural CJK text such as `请检查@审查会话` without opening mentions in emails. */
export function canStartMentionAt(text: string, index: number): boolean {
  if (index <= 0) return true
  const unit = text.charCodeAt(index - 1)
  const prefix =
    unit >= 0xdc00 && unit <= 0xdfff && index >= 2
      ? text.slice(index - 2, index)
      : text.charAt(index - 1)
  return ALLOWED_MENTION_PREFIX.test(prefix)
}

/** Tiptap's matcher with a Unicode-aware prefix check for unspaced scripts. */
export function findMentionMatch(config: Trigger): SuggestionMatch {
  const match = findSuggestionMatch({ ...config, allowedPrefixes: null })
  if (!match) return null

  const node = config.$position.nodeBefore
  const text = node?.isText ? (node.text ?? "") : ""
  const index = match.range.from - (config.$position.pos - text.length)
  return canStartMentionAt(text, index) ? match : null
}

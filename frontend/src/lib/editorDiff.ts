export type DiffKind = 'equal' | 'insert' | 'delete'

export interface DiffPart {
  kind: DiffKind
  text: string
}

const MAX_LCS_CELLS = 250_000

/** Small LCS diff used only for one entry history card; no heavyweight editor dependency needed. */
export function diffText(before: string, after: string, mode: 'character' | 'word'): DiffPart[] {
  const tokenize = mode === 'character' ? Array.from : wordTokens
  const left = tokenize(before)
  const right = tokenize(after)
  // History can contain long source payloads and up to hundreds of versions. Bound the quadratic
  // LCS table and retain a useful prefix/middle/suffix diff instead of freezing the editor tab.
  if (left.length * right.length > MAX_LCS_CELLS) return boundedLinearDiff(left, right)
  const table = Array.from({ length: left.length + 1 }, () => new Uint32Array(right.length + 1))
  for (let i = left.length - 1; i >= 0; i -= 1) {
    for (let j = right.length - 1; j >= 0; j -= 1) {
      table[i][j] =
        left[i] === right[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1])
    }
  }
  const parts: DiffPart[] = []
  let i = 0
  let j = 0
  const push = (kind: DiffKind, text: string) => {
    if (!text) return
    const previous = parts.at(-1)
    if (previous?.kind === kind) previous.text += text
    else parts.push({ kind, text })
  }
  while (i < left.length && j < right.length) {
    if (left[i] === right[j]) {
      push('equal', left[i] ?? '')
      i += 1
      j += 1
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      push('delete', left[i] ?? '')
      i += 1
    } else {
      push('insert', right[j] ?? '')
      j += 1
    }
  }
  while (i < left.length) push('delete', left[i++] ?? '')
  while (j < right.length) push('insert', right[j++] ?? '')
  return parts
}

function boundedLinearDiff(left: string[], right: string[]): DiffPart[] {
  let prefix = 0
  const sharedLength = Math.min(left.length, right.length)
  while (prefix < sharedLength && left[prefix] === right[prefix]) prefix += 1

  let suffix = 0
  while (
    suffix < sharedLength - prefix &&
    left[left.length - 1 - suffix] === right[right.length - 1 - suffix]
  ) {
    suffix += 1
  }

  const parts: DiffPart[] = []
  const push = (kind: DiffKind, tokens: string[]) => {
    const text = tokens.join('')
    if (text) parts.push({ kind, text })
  }
  push('equal', left.slice(0, prefix))
  push('delete', left.slice(prefix, left.length - suffix))
  push('insert', right.slice(prefix, right.length - suffix))
  if (suffix > 0) push('equal', left.slice(left.length - suffix))
  return parts
}

function wordTokens(value: string): string[] {
  return value.match(/[\p{L}\p{N}_]+|\s+|[^\p{L}\p{N}_\s]/gu) ?? []
}

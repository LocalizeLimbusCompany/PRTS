import DOMPurify from 'dompurify'
import MarkdownIt from 'markdown-it'

const markdown = new MarkdownIt({
  breaks: true,
  html: true,
  linkify: true,
  typographer: false,
})

const unsafeLinkProtocol = /^(?:javascript|vbscript|data)\s*:/i

/** Remove unsafe Markdown destinations while keeping their visible labels. */
function stripUnsafeMarkdownLinks(source: string): string {
  let output = ''
  let cursor = 0

  while (cursor < source.length) {
    const labelStart = source.indexOf('[', cursor)
    if (labelStart < 0) {
      output += source.slice(cursor)
      break
    }

    const labelEnd = source.indexOf('](', labelStart + 1)
    if (labelEnd < 0) {
      output += source.slice(cursor)
      break
    }

    let destinationEnd = labelEnd + 2
    let depth = 1
    while (destinationEnd < source.length && depth > 0) {
      if (source[destinationEnd] === '(') depth += 1
      if (source[destinationEnd] === ')') depth -= 1
      destinationEnd += 1
    }

    if (depth > 0) {
      output += source.slice(cursor)
      break
    }

    const destination = source.slice(labelEnd + 2, destinationEnd - 1).trim()
    if (!unsafeLinkProtocol.test(destination)) {
      output += source.slice(cursor, destinationEnd)
      cursor = destinationEnd
      continue
    }

    const imagePrefix = labelStart > 0 && source[labelStart - 1] === '!' ? 1 : 0
    output += source.slice(cursor, labelStart - imagePrefix)
    output += source.slice(labelStart + 1, labelEnd)
    cursor = destinationEnd
  }

  return output
}

/** Render user-authored Markdown and remove executable or unsafe markup. */
export function renderSafeMarkdown(source: string): string {
  return DOMPurify.sanitize(markdown.render(stripUnsafeMarkdownLinks(source)), {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ['style', 'iframe', 'object', 'embed'],
    FORBID_ATTR: ['style'],
  })
}

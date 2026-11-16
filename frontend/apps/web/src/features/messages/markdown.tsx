import { Fragment, type ReactNode } from 'react'
import type { Message, User } from '../../types'

/**
 * Renders the small supported Markdown subset (emphasis, inline code, fenced
 * code, lists, http(s) links and @mentions) as React elements. Nothing is
 * injected as HTML, and only http(s) URLs become links.
 */
export function renderMessageContent(message: Message, profiles: Record<string, User>) {
  const mentionedNames = new Set((message.mentioned_user_ids ?? [])
    .map((id) => profiles[id]?.username.toLowerCase())
    .filter((name): name is string => Boolean(name)))
  const inline = (value: string, keyPrefix: string): ReactNode[] => {
    const parts = value.split(/(`[^`]+`|\*\*[^*]+\*\*|~~[^~]+~~|\*[^*]+\*|\[[^\]]+\]\(https?:\/\/[^\s)]+\)|https?:\/\/[^\s<>()]+|@[A-Za-z0-9_]{3,32})/g)
    return parts.map((part, index) => {
      const key = `${keyPrefix}-${index}`
      if (part.startsWith('`') && part.endsWith('`')) return <code key={key}>{part.slice(1, -1)}</code>
      if (part.startsWith('**') && part.endsWith('**')) return <strong key={key}>{part.slice(2, -2)}</strong>
      if (part.startsWith('~~') && part.endsWith('~~')) return <del key={key}>{part.slice(2, -2)}</del>
      if (part.startsWith('*') && part.endsWith('*')) return <em key={key}>{part.slice(1, -1)}</em>
      const markdownLink = part.match(/^\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)$/)
      if (markdownLink) return <a key={key} href={markdownLink[2]} target="_blank" rel="noopener noreferrer">{markdownLink[1]}</a>
      if (/^https?:\/\//i.test(part)) {
        const suffix = part.match(/[.,!?;:]+$/)?.[0] ?? ''
        const url = suffix ? part.slice(0, -suffix.length) : part
        return <Fragment key={key}><a href={url} target="_blank" rel="noopener noreferrer">{url}</a>{suffix}</Fragment>
      }
      if (part.startsWith('@') && mentionedNames.has(part.slice(1).toLowerCase())) return <span className="mention-highlight" key={key}>{part}</span>
      return part
    })
  }

  const blocks: ReactNode[] = []
  const paragraphs: string[] = []
  let listItems: string[] = []
  let orderedList = false
  let codeLines: string[] = []
  let inCode = false
  const flushParagraph = () => {
    if (paragraphs.length) blocks.push(<p key={`p-${blocks.length}`}>{paragraphs.map((line, index) => <Fragment key={index}>{index > 0 && <br />}{inline(line, `p${blocks.length}-${index}`)}</Fragment>)}</p>)
    paragraphs.length = 0
  }
  const flushList = () => {
    if (listItems.length) {
      const List = orderedList ? 'ol' : 'ul'
      blocks.push(<List key={`list-${blocks.length}`}>{listItems.map((line, index) => <li key={index}>{inline(line, `list${blocks.length}-${index}`)}</li>)}</List>)
    }
    listItems = []
  }
  const lines = message.content.split('\n')
  for (const [index, line] of lines.entries()) {
    if (line.trimStart().startsWith('```')) {
      flushParagraph(); flushList()
      if (inCode) {
        blocks.push(<pre key={`code-${blocks.length}`}><code>{codeLines.join('\n')}</code></pre>)
        codeLines = []
      }
      inCode = !inCode
      continue
    }
    if (inCode) { codeLines.push(line); continue }
    const item = line.match(/^\s*([-*+]|\d+\.)\s+(.+)$/)
    if (item) {
      flushParagraph()
      const nextOrdered = /^\d/.test(item[1] ?? '')
      if (listItems.length && nextOrdered !== orderedList) flushList()
      orderedList = nextOrdered
      listItems.push(item[2] ?? '')
    } else if (!line.trim()) {
      flushParagraph(); flushList()
    } else {
      flushList(); paragraphs.push(line)
    }
    if (index === lines.length - 1) { flushParagraph(); flushList() }
  }
  if (inCode) blocks.push(<pre key={`code-${blocks.length}`}><code>{codeLines.join('\n')}</code></pre>)
  flushParagraph(); flushList()
  return <>{blocks}</>
}

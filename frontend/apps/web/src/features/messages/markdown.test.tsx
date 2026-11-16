import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import type { Message } from '../../types'
import { renderMessageContent } from './markdown'

const message = (content: string, extra: Partial<Message> = {}): Message => ({
  id: 'message-1', conversation_id: 'conversation-1', sender_id: 'user-1', sequence: 1, client_message_id: 'client-1', content, created_at: '', ...extra,
})

function html(content: string, profiles = {}, extra: Partial<Message> = {}) {
  const { container } = render(<div>{renderMessageContent(message(content, extra), profiles)}</div>)
  return container
}

describe('renderMessageContent', () => {
  it('renders emphasis, inline code and lists as elements', () => {
    const container = html('**bold** and *soft* and `code`\n\n- one\n- two')
    expect(container.querySelector('strong')?.textContent).toBe('bold')
    expect(container.querySelector('em')?.textContent).toBe('soft')
    expect(container.querySelector('code')?.textContent).toBe('code')
    expect([...container.querySelectorAll('li')].map((item) => item.textContent)).toEqual(['one', 'two'])
  })

  it('links only http(s) URLs and opens them safely', () => {
    const container = html('see https://example.test/page. and [x](javascript:alert(1)) and [ok](https://example.test)')
    const links = [...container.querySelectorAll('a')]
    expect(links.map((link) => link.getAttribute('href'))).toEqual(['https://example.test/page', 'https://example.test'])
    for (const link of links) {
      expect(link.getAttribute('rel')).toBe('noopener noreferrer')
      expect(link.getAttribute('target')).toBe('_blank')
    }
    expect(container.innerHTML).not.toContain('href="javascript')
  })

  it('shows HTML in a message as text instead of markup', () => {
    const container = html('<img src=x onerror=alert(1)> <script>alert(1)</script>')
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('script')).toBeNull()
    expect(container.textContent).toContain('<img src=x onerror=alert(1)>')
  })

  it('keeps fenced code verbatim', () => {
    const container = html('```\n**not bold**\n```')
    expect(container.querySelector('pre code')?.textContent).toBe('**not bold**')
    expect(container.querySelector('strong')).toBeNull()
  })

  it('highlights only mentions that resolved to members', () => {
    const profiles = { 'user-2': { id: 'user-2', username: 'bobby', created_at: '' } }
    const container = html('hi @bobby and @stranger', profiles, { mentioned_user_ids: ['user-2'] })
    expect([...container.querySelectorAll('.mention-highlight')].map((item) => item.textContent)).toEqual(['@bobby'])
  })
})

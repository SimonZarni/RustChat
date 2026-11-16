# Manual screen-reader review

Automated checks (axe-core in Vitest and Playwright, keyboard-focus tests) cover structure and contrast but cannot judge what a screen reader actually says. Phase 1 stays "in progress" until a person completes this review and records the result at the bottom of this file.

## Setup

- Windows: NVDA (free) with Firefox or Chrome. Optionally also Narrator with Edge.
- macOS/iOS: VoiceOver with Safari.
- Run the full stack (see [DEPLOYMENT.md](DEPLOYMENT.md)) or `pnpm dev` with the backend from the README. Use two accounts so there is a second person to message.
- Turn the screen off or close your eyes for the walkthrough; use only the keyboard and the screen reader.

## Walkthrough

For each step, note whether the spoken output identifies **what** the control is, its **name**, and its **state**, and whether focus lands somewhere sensible.

1. **Sign in and register.** Fields announce their labels; errors are announced when they appear (they use `role="alert"`); the success notice after registering is announced.
2. **Password recovery.** "Forgot password?" moves to the email form; the confirmation text is announced.
3. **Landmarks and headings.** Jump by landmark and heading: the workspace, both sidebars and the chat panel are reachable and named.
4. **New conversation (Ctrl/⌘ + K).** The dialog is announced with its title; focus is on the username field; Tab stays inside; Escape closes it and focus returns to the opener.
5. **Conversation list.** Each conversation reads as a button with its name; the online indicator reads "Online".
6. **Message history.** Entering the conversation reads the message log by name; each message has author, time and text; "Load older messages" is reachable first.
7. **Receiving a message.** A new message from the other account is announced once, politely, without moving focus. The "is typing…" status is announced without being repeated.
8. **Composer.** The field is named "Write a message"; Enter sends and Shift+Enter inserts a line; a failed send announces the error and keeps the text.
9. **Reactions and replies.** Reaction buttons announce add/remove, the emoji and the count, and read as pressed or not pressed; Reply names the message being replied to.
10. **Edit and delete.** The prompts are announced; a deleted message is announced as removed or tombstoned.
11. **Communities.** Create, join, channel list and the Manage dialog: group labels (categories) are announced, the dialog traps focus, and every row action names its target (for example "Suspend bobby").
12. **Settings.** Profile, Appearance, Security, Privacy and Account actions expand and collapse with state announced; destructive actions require the typed confirmation and say so.
13. **Reconnect.** Turn the network off and on: the "Reconnecting" / "Live" status is announced and does not steal focus.
14. **Zoom and reduced motion.** At 200 % zoom and with "reduce motion" enabled, nothing is clipped or animates.

## Recording the result

Add a dated entry below with the reviewer, the assistive technology and browser, the steps that passed, and every problem found (with an issue link). Fix problems before marking Phase 1 complete in [PHASE_STATUS.md](PHASE_STATUS.md), the README and ARCHITECTURE.md.

| Date | Reviewer | Technology | Result |
|---|---|---|---|
| _not yet performed_ | | | |

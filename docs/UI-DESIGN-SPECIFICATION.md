# RustChat — Frontend UI Design Specification

## 1. Product Design Direction

RustChat should have an original, understated, technically sophisticated interface.

The goal is not to recreate Discord or Slack. It is to create a communication product with its own identity: focused, functional, comfortable for long sessions, and visually consistent across web, desktop, and mobile.

### Design Principles
- Product-focused rather than dashboard-focused.
- Clear visual hierarchy without excessive decoration.
- Functional density without visual clutter.
- Consistent spacing, typography, and interaction patterns.
- Comfortable for extended reading and communication.
- Responsive across phones, tablets, laptops, Macs, and large monitors.
- Accessible by mouse, keyboard, and touch.

### Avoid
- Excessive gradients, glows, and glassmorphism.
- Generic AI-dashboard layouts.
- Too many rounded cards and pill-shaped controls.
- Decorative illustrations that do not serve a function.
- Emoji as interface icons.
- Unnecessary animations and transitions.
- Large empty spaces that reduce useful information density.

---

## 2. Visual Identity and Color System

**Selected Palette:** Obsidian & Slate

Use a restrained neutral color system with slate blue as the primary accent. The light and dark themes must feel like two versions of the same product, not separate designs.

### Light Theme
| Token | Color | Purpose |
| :--- | :--- | :--- |
| Background | `#F3F4F5` | Main application background |
| Surface | `#FFFFFF` | Primary panels |
| Secondary surface | `#E9EBED` | Subtle secondary areas |
| Primary text | `#202326` | Main content |
| Secondary text | `#737980` | Supporting text |
| Border | `#DDE0E3` | Dividers and outlines |
| Accent | `#596B80` | Primary interactive accent |

### Dark Theme
| Token | Color | Purpose |
| :--- | :--- | :--- |
| Background | `#151719` | Main application background |
| Surface | `#1E2225` | Primary panels |
| Secondary surface | `#292F34` | Subtle secondary areas |
| Primary text | `#E7E9EB` | Main content |
| Secondary text | `#A0A7AD` | Supporting text |
| Border | `#343A3F` | Dividers and outlines |
| Accent | `#8498AD` | Primary interactive accent |

### Color Usage Rules
- Neutral colors should dominate the interface.
- Use the accent for selected navigation, primary actions, links, focus indicators, and important interactive states.
- Do not use the accent as a decorative background everywhere.
- Use semantic success, warning, and danger colors only when needed.
- Semantic colors must be adapted for both themes.
- Validate text and interactive-state contrast against WCAG AA.
- Do not rely on color alone to communicate status.

### Theme Behavior
RustChat must support:
1. **Light mode**
2. **Dark mode**
3. **System mode** (following the operating system preference)

**Requirements:**
- Provide a theme selector in Settings and a convenient quick-access location.
- Persist the user's explicit preference across sessions.
- System mode should respond to operating-system theme changes.
- Apply theme changes immediately without a page reload.
- Avoid a flash of the wrong theme during initial loading.
- Use semantic design tokens rather than hardcoded colors in components.
- Ensure dialogs, menus, tooltips, notifications, editors, code blocks, and overlays work in both themes.
- Test images, avatars, status indicators, and selected states against both backgrounds.
- Theme changes must not reset the current conversation or navigation state.

---

## 3. Typography

### Recommended Initial Direction
- **IBM Plex Sans** for the main interface.
- **IBM Plex Mono** for technical identifiers, diagnostics, and code-related content.

### Guidelines
- Typography should prioritize readability and clear hierarchy.
- Use a consistent scale for:
  - Page and section headings.
  - Conversation and channel names.
  - Message content.
  - Supporting metadata.
  - Timestamps and labels.
- Avoid excessive font weights and unnecessarily large headings.
- The final font choice should be validated in actual conversation layouts before implementation.

---

## 4. Icons

Use **Lucide icons** as the initial icon foundation.

### Rules
- Consistent stroke width and visual weight.
- Consistent sizing.
- Tooltips for unfamiliar icon-only actions.
- No decorative icon overload.
- Avoid using emoji as replacements for functional icons.
- Provide accessible labels for icon-only controls.

---

## 5. Layout and Application Structure

The primary desktop workspace should contain:
1. Workspace navigation rail.
2. Conversation or channel sidebar.
3. Main conversation area.
4. Optional contextual panel.

### Initial Desktop Reference Dimensions
- **Workspace rail:** approximately `64px`
- **Sidebar:** approximately `248px`
- **Contextual panel:** approximately `280px`

*(Note: These are starting values, not fixed dimensions. Layout must adapt to available space.)*

### Main Workspace
The workspace should prioritize the active conversation:
- Keep navigation visually distinct from conversation content.
- Allow independent scrolling of navigation, message history, and contextual panels.
- Avoid unnecessary nested containers.
- Keep message content readable at wide resolutions.
- Support keyboard navigation and shortcuts.

### Conversation Layout
- Group consecutive messages from the same sender.
- Avoid putting every message inside a floating card.
- Make timestamps and sender information easy to scan.
- Provide clear distinction between messages, system events, and date separators.
- Keep message actions discoverable without requiring hover.
- Support long messages, links, code blocks, attachments, and replies.

### Navigation
Provide clear access to:
- Direct messages.
- Group conversations.
- Communities or servers.
- Channels.
- Search.
- Notifications.
- Settings and profile.

---

## 6. Responsive Design

RustChat must be fully responsive across phones, tablets, laptops, Macs, large monitors, and ultrawide displays. Breakpoints are guidelines, not rigid device categories.

| Width | Target |
| :--- | :--- |
| `320–374px` | Small mobile |
| `375–430px` | Standard mobile |
| `431–599px` | Large mobile |
| `600–767px` | Small tablet |
| `768–1023px` | Tablet |
| `1024–1279px` | Small laptop |
| `1280–1439px` | Standard laptop |
| `1440–1919px` | Large laptop / Mac |
| `1920–2559px` | Large monitor |
| `2560px+` | Ultrawide |

### Mobile
- Show one primary navigation or content panel at a time.
- Use a clear navigation drawer or view-switching pattern.
- Keep conversation lists and active conversations as separate navigable views.
- Keep the message composer accessible above the on-screen keyboard.
- Make contextual panels full-screen views or bottom sheets.
- Use touch targets of at least `44 × 44` CSS pixels where practical.
- Never depend on hover to access essential actions.
- Prevent accidental horizontal overflow.
- Respect device safe areas.

### Tablet
- Allow navigation to collapse.
- Make the conversation sidebar toggleable.
- Use a drawer for contextual panels when space is limited.
- Support portrait and landscape orientations.

### Laptop and Desktop
- Preserve the multi-panel workspace when space permits.
- Allow sidebars to collapse or resize.
- Keep the message area comfortable to read.
- Avoid requiring excessive scrolling for frequently used controls.

### Large Monitors and Ultrawide
- Use additional width for useful panels and navigation, not stretched message lines.
- Control the maximum readable width of message content.
- Maintain sensible spacing and panel proportions.

### Implementation Requirements
- Use CSS Grid and Flexbox.
- Prefer content-driven breakpoints.
- Use `minmax()`, `clamp()`, and container queries where appropriate.
- Support dynamic mobile viewport heights.
- Handle virtual keyboard resizing.
- Respect safe-area insets.
- Test browser zoom and increased text size.

### Desktop Application (Tauri)
The Tauri application must support Windows, macOS, and Linux:
- Support resizable windows.
- Define practical minimum window dimensions.
- Respect platform window conventions.
- Support keyboard shortcuts.
- Ensure the UI remains usable in compact windows.

### Responsive Test Viewports
At minimum, test:
- `320 × 568`
- `375 × 812`
- `390 × 844`
- `430 × 932`
- `768 × 1024`
- `1024 × 768`
- `1280 × 800`
- `1366 × 768`
- `1440 × 900`
- `1728 × 1117`
- `1920 × 1080`
- `2560 × 1440`
- `3440 × 1440`

*(Also test intermediate widths to catch layout transitions that only fail between standard device sizes.)*

---

## 7. Component Foundation

Use **shadcn/ui** as a starting component foundation, customized to fit RustChat's identity.

### Initial Shared Components
- `Button`
- `IconButton`
- `Input`
- `Textarea`
- `Avatar`
- `PresenceIndicator`
- `Tooltip`
- `DropdownMenu`
- `ContextMenu`
- `Dialog`
- `Popover`
- `Tabs`
- `Toast`
- `Badge`
- `Separator`
- `Skeleton`
- `MessageItem`
- `MessageComposer`
- `ConversationListItem`
- `ChannelListItem`
- `MemberListItem`

### Component Rules
- Components must use shared design tokens and support both themes.
- Do not accept default shadcn styling without reviewing it against the product design.

---

## 8. Interaction and Motion

Motion should communicate state changes and improve usability.
- Keep transitions subtle and purposeful.
- Avoid unnecessary bounce and spring effects.
- Use consistent transition durations.
- Respect `prefers-reduced-motion`.
- Provide clear hover, focus, active, selected, disabled, and loading states.
- Ensure touch interactions have appropriate feedback.
- Do not make important information dependent on animation.

---

## 9. Frontend Architecture

### Core Stack

> **Status:** this is the target stack. The implemented frontend currently uses React, TypeScript, Vite, plain CSS and React state with `fetch`; Tailwind CSS, shadcn/ui, TanStack Query, Zustand and Zod are not yet dependencies (see [ARCHITECTURE.md](ARCHITECTURE.md)). Adopt them only when a concrete need justifies it.

- React
- TypeScript
- Vite
- Tailwind CSS
- shadcn/ui
- TanStack Query
- Zustand
- Zod
- Tauri 2 for desktop

### State Management
**Use TanStack Query for remote/server state:**
- Conversations
- Messages
- Members
- Channels
- User profiles
- Server data

**Use Zustand for local UI state:**
- Active navigation panel
- Open drawers
- Selected conversation
- Composer UI state
- Temporary interface preferences

*(Avoid duplicating server state in Zustand.)*

### Shared Application
- Use one shared React frontend for web and Tauri desktop.
- Do not create separate business logic or duplicate frontend implementations for each platform.

---

## 10. Testing and Quality

### Initial Testing Stack
- Vitest
- React Testing Library
- MSW
- Playwright

### Test Coverage Requirements
- Both themes (Light and Dark).
- All responsive layout ranges.
- Keyboard navigation.
- Touch interactions.
- Loading and error states.
- Long conversations and large message histories.
- Empty states.
- Authentication and permission-related UI.
- Accessibility and contrast.
- Desktop window resizing.

### Definition of Done
A screen is not complete until:
- [ ] It works in light and dark mode.
- [ ] It works at mobile, tablet, laptop, and large-monitor sizes.
- [ ] It has no accidental horizontal overflow.
- [ ] Essential features remain accessible at every supported size.
- [ ] Mouse, keyboard, and touch interactions work.
- [ ] Loading, empty, error, focus, and disabled states are designed.
- [ ] Accessibility and contrast have been checked.

---

## 11. Design Implementation Rule

Before implementing a screen, define:
1. Its purpose and information hierarchy.
2. Its desktop layout.
3. Its mobile layout.
4. Its light and dark appearance.
5. Its interaction states.
6. Its loading, empty, and error states.
7. Its accessibility requirements.

Do not start by generating generic pages or copying a familiar chat application's layout. Build a coherent design system first, then implement screens using that system.
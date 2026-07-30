# Design QA: Chat app starter

## Evidence

- Source screenshot: `/var/folders/xr/0499tfdd1xlg4nv59qbwcxzm0000gn/T/TemporaryItems/NSIRD_screencaptureui_BEiPae/Screenshot 2026-07-29 at 7.24.10 PM.png`
- Implementation screenshot: `/tmp/sixpack-chat-app.png`
- Side-by-side comparison: `/tmp/sixpack-chat-comparison.png`
- Source image: 1331 × 888 pixels
- Implementation viewport and image: 1408 × 880 CSS pixels at device scale 1
- Compared state: 6 stored messages, revision 3, three user/assistant exchanges

## Full-view comparison

The implementation preserves the supplied black, white, gray, and lime visual
system, the large heading scale, the four-metric strip, the two-column layout,
thin borders, and restrained type weights. It applies the requested content and
interaction changes: the title is now “Chat app,” the fake-endpoint language
and local badge are gone, and the primary workspace is a chat beside a compact
database table.

## Focused regions

### Chat

- The composer is pinned to the bottom of the panel.
- User messages are compact lime rectangles aligned right.
- Assistant messages are outlined dark rectangles aligned left.
- New bubbles animate once; periodic synchronization does not replay the
  animation.

### Database state

- The previous stacked cards are replaced with a two-column role/message table.
- Six rows fit in roughly half the vertical space of the supplied state panel.
- IDs, serial values, and timestamps are absent.
- Both rows from a chat exchange receive the incoming-row animation.

## Typography, spacing, color, and copy

- Heading remains the only heavy typographic element.
- Supporting labels and values use thin or regular weights.
- Panel padding and row height are compact and consistent.
- Lime remains limited to metrics, user bubbles, actions, and role labels.
- No image assets are used.
- “AI Ping,” “fake local endpoint,” and “pong” are removed from the visible
  product language and generated behavior.

## Verification

- Real-browser form submission produced one user bubble and one assistant
  bubble.
- The same submission advanced the database from 6 to 8 rows and revision 3 to
  revision 4 without a reload.
- Browser-visible response and database-write timing values populated in
  floored integer microseconds.
- Generated-project unit test confirmed `ping` stores `user: ping` and
  `assistant: ping`.

## Result

Passed. No P0, P1, or P2 visual discrepancies remain after accounting for the
intentional layout and copy changes requested for the chat and database panels.

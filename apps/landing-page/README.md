# sixpack Site

Minimal Astro landing page and docs shell for the public sixpack project.

```sh
bun install
bun run dev
```

The site intentionally mirrors current repository behavior. Avoid describing
planned features as implemented.

## Add a docs page

Create a Markdown file in `src/content/docs`:

```md
---
title: Page title
description: One-sentence page summary.
order: 7
---

Write the page in Markdown.
```

The content collection validates the frontmatter. The shared docs route adds
the page at `/docs/<filename>` and places it in the sidebar by `order`.

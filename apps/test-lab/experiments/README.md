# Test Lab Experiments

This folder holds experimental notes for work that is not part of the core runtime.

## How experiments are managed

- Keep active work in short, throwaway notes files:
  - `apps/test-lab/experiments/<date>-<topic>.md`
- Keep fixture/reference inputs in `apps/test-lab/fixtures/`.
- Keep completed experiment summaries outside the public repository.

## Static data projection

`packages/sixpack/assets/data-projection.html` is the template for single-file,
read-only browser projections used in local timing and revision-sync checks. It
has no frontend dependencies, server, or write path.

The note init experiment creates `<database>/projection.html` as soon as it
initializes a database. The note playground rewrites it after each mutation,
and the AI-chat demo rewrites it after each persistence step. Open the generated
file and the current rows appear immediately; no database upload or browser
filesystem permission is required. While open, it checks once per second so a
rewritten projection becomes visible without losing the selected table or
search.

The template itself intentionally contains no database.

## Suggested compact summary format

```md
# [Topic]
- Goal: ...
- Method: ...
- Inputs: ...
- Results: ...
- Decision: ...
- Follow-up: ...
```

When an experiment is archived, delete or move noisy scratch notes to avoid clutter in active paths.

# Research Decisions

- Parser strategy: adapter boundary keeps snapshot version churn isolated.
- Core language: Rust for stronger typing and safer low-level handling.
- Adapter language: Python for fast per-hash parser updates.
- v1 scope: Android ARM64 static-only, correctness prioritized over breadth.
- Quality gates: strict defaults to prevent low-confidence pseudocode output.

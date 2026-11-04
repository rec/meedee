# Python and Rust API parity

Keep the Python and Rust public APIs as similar as their languages allow.

- Review both APIs whenever changing a public interface. Implement the corresponding
  change in both languages within the same task unless the user approves an exception.
- Align public module and symbol names, supported operations, defaults, validation,
  units, lifecycle behavior, error information and report contents. Matching names
  alone do not establish parity.
- Preserve idiomatic language differences: Python keyword arguments, callable output
  and exceptions; Rust builders/options, typed durations, enums, traits, ownership
  and Result. These differences should express the same underlying behavior.
- Keep the existing Python-only CLI, process isolation, unit-string parsing and uFor
  integration exceptions explicit. Ask before introducing further API divergence
  or choosing a behavior-changing design to resolve a mismatch.
- Update both languages' examples and relevant tests when changing shared behavior.
  Verify the corresponding operation and edge cases, not just symbol counts.
- Replace superseded interfaces and migrate callers rather than retaining parallel
  legacy APIs unless the user explicitly requests compatibility.
- Keep public-symbols.txt uncommitted unless the user explicitly requests otherwise.

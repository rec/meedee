## Python and Rust API parity

Keep the Python and Rust public APIs as similar as their languages allow.

- Review both APIs whenever changing a public interface. Implement the corresponding
  change in both languages within the same task unless the user approves an exception.
- Align public module and symbol names, supported operations, defaults, validation,
  units, lifecycle behavior, error information and report contents. Matching names
  alone do not establish parity.
- Preserve idiomatic language differences: Python keyword arguments and exceptions;
  Rust builders/options, typed durations, enums, traits, ownership and Result. These differences should express the same underlying behavior.
- Keep the existing Python-only CLI, unit-string parsing and uFor
  integration exceptions explicit. Ask before introducing further API divergence
  or choosing a behavior-changing design to resolve a mismatch.
- Update both languages' examples and relevant tests when changing shared behavior.
  Verify the corresponding operation and edge cases, not just symbol counts.
- Replace superseded interfaces and migrate callers rather than retaining parallel
  legacy APIs unless the user explicitly requests compatibility.
- Keep public-symbols.txt uncommitted unless the user explicitly requests otherwise.

## doc/examples*.md

Please don't erase human written comments there, and please do not add your
own comments. Only make changes to the Rust code and the Python code, and then the minimum changes to the text that are required, if any.

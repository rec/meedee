# Open implementation and qualification issues

Additional work beyond the prompt

None.

This list retains unfinished work and unqualified failure cases. Implemented
API behavior and completed remediation histories are not backlog items.
See [qualification](qualification.md) and
[libremidi provenance](../crates/middy/third_party/libremidi/SOURCE.md).

## Native backend coverage

- Windows native qualification remains open. Compile/run WinMM teardown, output
  and ANSI/Unicode discovery regressions, then test agreed loopback routes and
  installed wheels.
- Windows MIDI Services SDK/tooling integration is unfinished; the backend
  remains disabled in the native build. Native UMP qualification is separate
  from WinMM MIDI 1 checks.
- Linux ALSA sequencer/raw and available UMP backends need native, installed-wheel
  and runtime evidence. Mac virtual loopbacks do not qualify them.
- Other macOS versions/architectures and physical MIDI 1/UMP interoperability
  require separate evidence.

Checked C API teardown covers the patched input paths and CoreMIDI output
disposal. Direct C++ destruction, client-level CoreMIDI disposal errors and other
output backends remain outside that guarantee. The extent of OS resources left
after ignored disposal errors has not been measured.

## Failure and resource qualification

Physical unplug/replug, instrument reboot, driver/service crashes, unresponsive
hardware and resource exhaustion remain unqualified across platforms.
Agree named equipment, traffic and recovery procedures before testing.

Measure parent/child processes, idle/peak memory, CPU, threads, handles and
scheduling tails for the intended deployment. Native/client/queue limits do not
provide a machine-wide budget. Parameter caches and retained recordings can grow
with the workload. Normal and overload acceptance need separate evidence.

Slow callbacks, error hooks and in-process native calls can block close and
interpreter shutdown. Isolation supplies native-operation deadlines but cannot
cancel parent application code, a blocked filesystem or the OS MIDI service.
Reinitializing an embedded main interpreter after finalization remains
unqualified because the cleanup registry is process-wide.

## Deferred interoperability

MIDI-CI Profiles, Property Exchange and Process Inquiry engines remain deferred.
Native Linux/Windows UMP and physical MIDI-CI interoperability need qualification
independently of codecs and simulated exchanges. MIDI Clip profile negotiation
and additional manufacturer dump scopes also remain open. See
[feature proposals](features.md) and [SysEx coverage](../doc/sysex-dumps.md).

## Upstream review reminder

At project completion, remind Tom to send the libremidi fork commits upstream
for review, including regressions and validation limitations. The commits are
recorded in [source provenance](../crates/middy/third_party/libremidi/SOURCE.md).
Do not submit patches or contact maintainers automatically.

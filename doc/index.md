# middy

middy is a Rust MIDI toolkit with Python bindings for sending, receiving,
recording and processing MIDI on macOS, Windows and Linux. It supports MIDI 1
and MIDI 2.0/UMP, with tools for playback, routing, SysEx transfers and tuning.

Start with the [practical Python examples](examples.md) or
[practical Rust examples](rust-examples.md). These examples
progress from playing a melody on a synthesizer to recording performances,
transferring instrument dumps and discovering MIDI-CI devices.

For detailed transfer behavior, see [opaque SysEx dumps](sysex-dumps.md).
The [archive format](archive-format.md) describes `.middy` files, and
[specification baselines](specifications.md) records codec/vector provenance.

For module, class and function details, use the Python stubs and generated Rust
documentation. Installation and source-build requirements are in the
[build instructions](https://github.com/rec/middy/blob/main/README.ai.md#build-and-qualification).

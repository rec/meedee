# Vendored libremidi source

Repository: `git@github.com:rec/libremidi.git`

Revision: `07c7cfb1237be1ddeaa097a81f79cc9456c087ee`

Imported 2026-10-13 from the fork's `master` branch. This is a commit pin,
not the previously audited upstream v5.4.3 release archive. The fork already
contained other changes at base `996a73c216756de2dd6d58e66245ecae4830f9ff`.

The fork fixes are separate commits:

- [69e060e61b2d4962f283a9838f3307edb050a9bd](https://github.com/rec/libremidi/commit/69e060e61b2d4962f283a9838f3307edb050a9bd):
  reject failed integer-property queries; report a discovery error and omit
  the port rather than inventing an ID.
- [bf6f3cbfb50d8e22649511b1adb4e7e8eb012be9](https://github.com/rec/libremidi/commit/bf6f3cbfb50d8e22649511b1adb4e7e8eb012be9):
  convert name properties to checked, length-aware UTF-8 data with automatic
  Core Foundation ownership; remove the fixed buffer and C-string assumption.

- [debce21b71838cf205749ceafd9a17da397af823](https://github.com/rec/libremidi/commit/debce21b71838cf205749ceafd9a17da397af823):
  propagate endpoint/port disposal failure, retain callback-bearing ports after
  failed connection setup, and preserve the input handle through the C API until
  checked cleanup succeeds. A failed `midi_in_new` may return a non-null handle;
  callers must retain both it and callback context until `midi_in_free` succeeds.
  Direct C++ destruction is outside this checked C API contract.
- [56eedef9c6b6d8a5a4cc5bed0d4bd2cdaf08d209](https://github.com/rec/libremidi/commit/56eedef9c6b6d8a5a4cc5bed0d4bd2cdaf08d209):
  scope the explicit pre-delete close to CoreMIDI. Other backends retain their
  existing destructor behavior; this patch does not certify their teardown.

- [0932a62c05117a8f9b9d9b86ebac326c059531ba](https://github.com/rec/libremidi/commit/0932a62c05117a8f9b9d9b86ebac326c059531ba):
  extend checked input deletion to WinMM and Windows MIDI Services. A shared
  callback-lifetime guard rejects self-close, suppresses retired delivery and
  retains the object until active callbacks finish. WinMM can finish cleanup
  after its device handle has closed; MIDI Services records revoked/disconnected
  state and clears its endpoint only after draining, making success idempotent.

- [07c7cfb1237be1ddeaa097a81f79cc9456c087ee](https://github.com/rec/libremidi/commit/07c7cfb1237be1ddeaa097a81f79cc9456c087ee):
  add the Windows-only `winmm_teardown_test` with test-local fake device calls.
  It covers failed close, successful native close with an active callback,
  repeated timeout, rejected reopen and callback self-close. This target has
  not been compiled or run on this Mac; Windows validation is required.

No additional local modifications are applied to the imported files. Included
paths are `CMakeLists.txt`, `cmake/`, `include/`, `src/`, `LICENSE.md`, `README.md`,
`tests/include_catch.hpp`, `tests/unit/`, and `tests/integration/`. Upstream
examples, documentation, CI files, and the large MIDI file test corpus are
omitted. These sources support the planned native build and the focused
CoreMIDI regression target; this subset does not promise the full upstream
test suite or examples. Tests remain disabled for production builds.
`SOURCE.md` and `.gitattributes` are import metadata; the latter preserves
upstream line endings and whitespace without relaxing checks on our metadata.

The upstream license texts are retained in [LICENSE.md](LICENSE.md). Preserve
their notices when linking this source into distributed wheels and binaries.

## Regression checks

`tests/unit/coremidi_properties.cpp` supplies deterministic CoreMIDI property
getter stubs. A failed integer query does not write its output. String values
are real Core Foundation objects; conversion tests cover long ASCII, multibyte
UTF-8, embedded NUL, empty values and an unpaired UTF-16 surrogate. The observer
failure path is tested without creating MIDI clients or querying devices.

The property target passes 30 assertions in four test cases. The teardown target
passes 40 assertions in three test cases, using fake CoreMIDI clients, ports and
endpoints. It checks failure retention, repeated explicit cleanup, a still-live
callback after failure, successful deletion, and both failed-open rollback paths.
The portable callback-lifetime target adds 14 assertions in three cases for
self-close, nested/overlapping callbacks, drain failure and reopening after drain.
All three targets pass on macOS arm64, and all three also pass with AddressSanitizer and UndefinedBehaviorSanitizer. These checks
exercise property handling, CoreMIDI disposal faults and the shared lifetime
logic. They do not compile or execute the Windows backend headers: native WinMM
and Windows MIDI Services build/test qualification remains outstanding. They do
not establish whole-backend safety or hardware I/O.

Example focused build from the project root:

```sh
cmake -S third_party/libremidi -B target/libremidi-properties -G Ninja \
  -DLIBREMIDI_TESTS=ON -DLIBREMIDI_NO_BOOST=ON \
  -DLIBREMIDI_NO_JACK=ON -DLIBREMIDI_NO_PIPEWIRE=ON \
  -DLIBREMIDI_NO_NETWORK=ON -DLIBREMIDI_NO_KEYBOARD=ON \
  -DCMAKE_BUILD_TYPE=Debug -DCMAKE_OSX_DEPLOYMENT_TARGET=11.0
cmake --build target/libremidi-properties --target coremidi_properties_test coremidi_teardown_test callback_lifetime_test
ctest --test-dir target/libremidi-properties \
  -R '^(coremidi_properties|coremidi_teardown|callback_lifetime)_test$' --output-on-failure
```

The existing upstream test configuration obtains Catch2 when tests are enabled.
Production build integration must disable tests and avoid unpinned build-time
downloads. Native backend replacement and packaging integration are still the
remaining work of meedee slice 2; source vendoring alone does not replace midir.

#include "../include_catch.hpp"
#include <libremidi/backends/winmm/helpers.hpp>

#include <future>

namespace
{
MMRESULT close_status = MMSYSERR_NOERROR;
int closes{};

MMRESULT WINAPI fake_open(LPHMIDIIN out, UINT, DWORD_PTR, DWORD_PTR, DWORD)
{
  *out = reinterpret_cast<HMIDIIN>(1);
  return MMSYSERR_NOERROR;
}
MMRESULT WINAPI fake_handle(HMIDIIN) { return MMSYSERR_NOERROR; }
MMRESULT WINAPI fake_prepare(HMIDIIN, LPMIDIHDR header, UINT)
{
  header->dwFlags |= MHDR_PREPARED;
  return MMSYSERR_NOERROR;
}
MMRESULT WINAPI fake_unprepare(HMIDIIN, LPMIDIHDR header, UINT)
{
  header->dwFlags &= ~MHDR_PREPARED;
  return MMSYSERR_NOERROR;
}
MMRESULT WINAPI fake_add_buffer(HMIDIIN, LPMIDIHDR, UINT) { return MMSYSERR_NOERROR; }
MMRESULT WINAPI fake_close(HMIDIIN)
{
  ++closes;
  return close_status;
}
}

// Only this test translation unit uses fake WinMM device calls. Real Windows
// synchronization primitives are used, but no MIDI device is opened.
#define midiInOpen fake_open
#define midiInStart fake_handle
#define midiInStop fake_handle
#define midiInReset fake_handle
#define midiInClose fake_close
#define midiInPrepareHeader fake_prepare
#define midiInUnprepareHeader fake_unprepare
#define midiInAddBuffer fake_add_buffer
#include <libremidi/backends/winmm/midi_in.hpp>
#undef midiInOpen
#undef midiInStart
#undef midiInStop
#undef midiInReset
#undef midiInClose
#undef midiInPrepareHeader
#undef midiInUnprepareHeader
#undef midiInAddBuffer

TEST_CASE("WinMM failed close retains the device for checked cleanup", "[winmm]")
{
  close_status = MMSYSERR_NOERROR;
  closes = 0;
  libremidi::midi_in_winmm input{
      libremidi::input_configuration{.on_message = [](const libremidi::message&) {}},
      libremidi::winmm_input_configuration{}};
  REQUIRE(input.do_open(0) == stdx::error{});
  close_status = MMSYSERR_ERROR;
  const auto failed = input.close_port();
  const auto reopened = input.do_open(0);
  close_status = MMSYSERR_NOERROR;
  const auto cleaned = input.close_port();
  REQUIRE(failed != stdx::error{});
  REQUIRE(reopened != stdx::error{});
  REQUIRE(cleaned == stdx::error{});
  REQUIRE(closes == 2);
  REQUIRE(input.close_port() == stdx::error{});
  REQUIRE(closes == 2);
}

TEST_CASE("WinMM retains callback resources after device close until draining", "[winmm]")
{
  close_status = MMSYSERR_NOERROR;
  closes = 0;
  std::promise<void> entered, release;
  auto released = release.get_future();
  libremidi::midi_in_winmm input{
      libremidi::input_configuration{.on_message = [&](const libremidi::message&) {
        entered.set_value();
        released.wait();
      }}, libremidi::winmm_input_configuration{}};
  REQUIRE(input.do_open(0) == stdx::error{});
  std::thread callback{[&] {
    input.midiInputCallback(nullptr, MIM_DATA, reinterpret_cast<DWORD_PTR>(&input),
                            0x00643C90, 1);
  }};
  entered.get_future().wait();
  const auto first = input.close_port();
  const auto second = input.close_port();
  const auto prematurely_reopened = input.do_open(0);
  release.set_value();
  callback.join();
  REQUIRE(first != stdx::error{});
  REQUIRE(second != stdx::error{});
  REQUIRE(prematurely_reopened != stdx::error{});
  REQUIRE(closes == 1);
  REQUIRE(input.close_port() == stdx::error{});
  REQUIRE(closes == 1);
}

TEST_CASE("WinMM rejects callback self-close before native disposal", "[winmm]")
{
  close_status = MMSYSERR_NOERROR;
  closes = 0;
  libremidi::midi_in_winmm* input_ptr{};
  stdx::error callback_result{};
  libremidi::midi_in_winmm input{
      libremidi::input_configuration{.on_message = [&](const libremidi::message&) {
        callback_result = input_ptr->close_port();
      }}, libremidi::winmm_input_configuration{}};
  input_ptr = &input;
  REQUIRE(input.do_open(0) == stdx::error{});
  input.midiInputCallback(nullptr, MIM_DATA, reinterpret_cast<DWORD_PTR>(&input),
                          0x00643C90, 1);
  REQUIRE(callback_result != stdx::error{});
  REQUIRE(closes == 0);
  REQUIRE(input.close_port() == stdx::error{});
  REQUIRE(closes == 1);
}

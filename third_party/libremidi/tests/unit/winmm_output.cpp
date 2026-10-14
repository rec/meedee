#include "../include_catch.hpp"
#include <libremidi/backends/winmm/helpers.hpp>

#include <array>

namespace
{
MMRESULT open_status = MMSYSERR_NOERROR;
int closes{};
DWORD sent{};

MMRESULT WINAPI fake_output_open(LPHMIDIOUT out, UINT, DWORD_PTR, DWORD_PTR, DWORD)
{
  if (open_status == MMSYSERR_NOERROR)
    *out = reinterpret_cast<HMIDIOUT>(1);
  return open_status;
}
MMRESULT WINAPI fake_output_close(HMIDIOUT)
{
  ++closes;
  return MMSYSERR_NOERROR;
}
MMRESULT WINAPI fake_short_message(HMIDIOUT, DWORD packet)
{
  sent = packet;
  return MMSYSERR_NOERROR;
}
}

// Only this translation unit substitutes device calls. Do not link the
// production backend's different inline definition into this executable.
#define midiOutOpen fake_output_open
#define midiOutClose fake_output_close
#define midiOutShortMsg fake_short_message
#include <libremidi/backends/winmm/midi_out.hpp>
#undef midiOutOpen
#undef midiOutClose
#undef midiOutShortMsg

TEST_CASE("WinMM unopened and failed-open output does not close a handle", "[winmm]")
{
  closes = 0;
  {
    libremidi::midi_out_winmm output{{}, {}};
  }
  REQUIRE(closes == 0);
  open_status = MMSYSERR_ERROR;
  {
    libremidi::midi_out_winmm output{{}, {}};
    REQUIRE(output.do_open(0) != stdx::error{});
  }
  REQUIRE(closes == 0);
  open_status = MMSYSERR_NOERROR;
}

TEST_CASE("WinMM short output zeroes every unused DWORD byte", "[winmm]")
{
  open_status = MMSYSERR_NOERROR;
  closes = 0;
  {
    libremidi::midi_out_winmm output{{}, {}};
    REQUIRE(output.do_open(0) == stdx::error{});
    const std::array<uint8_t, 1> clock{0xf8};
    const std::array<uint8_t, 2> program{0xc0, 7};
    const std::array<uint8_t, 3> note{0x90, 60, 100};
    REQUIRE(output.send_message(clock.data(), clock.size()) == stdx::error{});
    REQUIRE(sent == 0x000000f8);
    REQUIRE(output.send_message(program.data(), program.size()) == stdx::error{});
    REQUIRE(sent == 0x000007c0);
    REQUIRE(output.send_message(note.data(), note.size()) == stdx::error{});
    REQUIRE(sent == 0x00643c90);
  }
  REQUIRE(closes == 1);
}

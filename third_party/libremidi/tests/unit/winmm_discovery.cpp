#include "../include_catch.hpp"
#include <libremidi/backends/winmm/helpers.hpp>

#include <algorithm>
#include <array>
#include <atomic>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace
{
using name = std::basic_string<TCHAR>;
std::vector<std::optional<name>> names;

UINT WINAPI fake_input_count() { return static_cast<UINT>(names.size()); }
UINT WINAPI fake_output_count() { return static_cast<UINT>(names.size()); }

MMRESULT WINAPI fake_input_caps(UINT_PTR index, LPMIDIINCAPS caps, UINT)
{
  if (index >= names.size() || !names[index])
    return MMSYSERR_BADDEVICEID; // Deliberately leave the output untouched.
  *caps = {};
  caps->wMid = 123;
  caps->wPid = static_cast<WORD>(index);
  std::copy(names[index]->begin(), names[index]->end(), caps->szPname);
  return MMSYSERR_NOERROR;
}
MMRESULT WINAPI fake_output_caps(UINT_PTR index, LPMIDIOUTCAPS caps, UINT)
{
  if (index >= names.size() || !names[index])
    return MMSYSERR_BADDEVICEID;
  *caps = {};
  caps->wMid = 456;
  caps->wPid = static_cast<WORD>(index);
  std::copy(names[index]->begin(), names[index]->end(), caps->szPname);
  return MMSYSERR_NOERROR;
}
}

// No real device query or observer polling thread is used in this executable.
// The helpers above include the Windows declarations before call substitution.
#undef midiInGetDevCaps
#undef midiOutGetDevCaps
#define midiInGetDevCaps fake_input_caps
#define midiOutGetDevCaps fake_output_caps
#define midiInGetNumDevs fake_input_count
#define midiOutGetNumDevs fake_output_count
#include <libremidi/backends/winmm/observer.hpp>
#undef midiInGetDevCaps
#undef midiOutGetDevCaps
#undef midiInGetNumDevs
#undef midiOutGetNumDevs

TEST_CASE("WinMM discovery reports failed queries and names only successful snapshots", "[winmm]")
{
  names = {std::nullopt, name{TEXT("Repeated")}, std::nullopt,
           name{TEXT("Repeated")}, name{TEXT("Unique")}, std::nullopt};
  std::vector<std::string> errors;
  libremidi::observer_winmm observer{
      {.on_error = [&](std::string_view text, const libremidi::source_location&) {
        errors.emplace_back(text);
      }}, {}};
  const auto inputs = observer.get_input_ports();
  REQUIRE(inputs.size() == 3);
  const auto outputs = observer.get_output_ports();
  REQUIRE(outputs.size() == 3);
  const std::array<std::size_t, 3> indexes{1, 3, 4};
  for (std::size_t i = 0; i < 3; ++i)
  {
    const auto index = indexes[i];
    REQUIRE(inputs[i].port == index);
    REQUIRE(outputs[i].port == index);
    const auto& input_device = std::get<libremidi::usb_device_identifier>(inputs[i].device);
    const auto& output_device = std::get<libremidi::usb_device_identifier>(outputs[i].device);
    REQUIRE(input_device.vendor_id == 123);
    REQUIRE(output_device.vendor_id == 456);
    REQUIRE(input_device.product_id == index);
    REQUIRE(output_device.product_id == index);
    REQUIRE(inputs[i].port_name == (i < 2 ? "Repeated" : "Unique"));
    REQUIRE(inputs[i].display_name
            == (i == 0 ? "Repeated 1" : i == 1 ? "Repeated 2" : "Unique 1"));
    REQUIRE(outputs[i].display_name == inputs[i].display_name);
  }
  REQUIRE(errors.size() == 6);
  REQUIRE(errors.front().find("input port 0 capabilities:") != std::string::npos);
  REQUIRE(errors.back().find("output port 5 capabilities:") != std::string::npos);

  errors.clear();
  names = {std::nullopt};
  REQUIRE(observer.get_input_ports().empty());
  REQUIRE(observer.get_output_ports().empty());
  REQUIRE(errors.size() == 2);
  names = {name{TEXT("Recovered")}};
  REQUIRE(observer.get_input_ports().front().display_name == "Recovered 1");
  REQUIRE(observer.get_output_ports().front().display_name == "Recovered 1");
}

TEST_CASE("WinMM error callbacks can reenter discovery without recursion", "[winmm]")
{
  names = {std::nullopt};
  int errors{};
  libremidi::observer_winmm observer{{}, {}};
  observer.configuration.on_error = [&](std::string_view, const libremidi::source_location&) {
    ++errors;
    REQUIRE(observer.get_input_ports().empty());
  };
  REQUIRE(observer.get_input_ports().empty());
  REQUIRE(errors == 1);
  REQUIRE(observer.get_input_ports().empty());
  REQUIRE(errors == 2);
}

TEST_CASE("WinMM concurrent query errors do not invoke the hook concurrently", "[winmm]")
{
  names = {std::nullopt};
  std::atomic_bool entered{};
  std::atomic_bool release{};
  std::atomic_int errors{};
  libremidi::observer_winmm observer{
      {.on_error = [&](std::string_view, const libremidi::source_location&) {
        ++errors;
        entered.store(true, std::memory_order_release);
        while (!release.load(std::memory_order_acquire))
          std::this_thread::yield();
      }}, {}};
  std::thread first{[&] { observer.get_input_ports(); }};
  while (!entered.load(std::memory_order_acquire))
    std::this_thread::yield();
  // Complete a second enumeration while the first callback is still active.
  const auto outputs = observer.get_output_ports();
  release.store(true, std::memory_order_release);
  first.join();
  REQUIRE(outputs.empty());
  REQUIRE(errors == 1);
}

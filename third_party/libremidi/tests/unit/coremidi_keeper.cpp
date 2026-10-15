#include "../include_catch.hpp"
#include <libremidi/backends/coremidi/midi_in.hpp>
#include <libremidi/backends/coremidi/midi_out.hpp>
#include <libremidi/backends/coremidi/observer.hpp>
#include <libremidi/backends/coremidi_ump/midi_in.hpp>
#include <libremidi/backends/coremidi_ump/midi_out.hpp>

#include <atomic>
#include <map>
#include <mutex>
#include <thread>
#include <vector>

namespace
{
std::mutex native_mutex;
std::map<MIDIClientRef, bool> clients;
MIDIClientRef next_client = 100;
int keeper_attempts{};
int keeper_disposals{};
int invalid_disposals{};
bool fail_keeper = true;
bool fail_owned{};
bool server_stopped{};
bool keeper_has_callback{};
}

// Native client calls only are replaced. If every client is disposed, this fake
// models Apple's documented inability to create another client in that process.
extern "C" OSStatus MIDIClientCreate(
    CFStringRef name, MIDINotifyProc notify, void* context, MIDIClientRef* output)
{
  std::lock_guard lock{native_mutex};
  const bool keeper = CFEqual(name, CFSTR("libremidi client keeper"));
  if (keeper)
  {
    ++keeper_attempts;
    keeper_has_callback |= notify != nullptr || context != nullptr;
    if (fail_keeper)
    {
      fail_keeper = false;
      return kMIDIUnknownError; // Leave output untouched.
    }
  }
  else if (fail_owned)
  {
    fail_owned = false;
    return kMIDIUnknownError;
  }
  if (server_stopped)
    return kMIDIServerStartErr;
  *output = next_client++;
  clients.emplace(*output, keeper);
  return noErr;
}

extern "C" OSStatus MIDIClientDispose(MIDIClientRef client)
{
  std::lock_guard lock{native_mutex};
  const auto found = clients.find(client);
  if (found == clients.end())
  {
    ++invalid_disposals;
    return kMIDIInvalidClient;
  }
  keeper_disposals += found->second;
  clients.erase(found);
  if (clients.empty())
    server_stopped = true;
  return noErr;
}

TEST_CASE("CoreMIDI keeps one callback-free client across failure, concurrency and reopen", "[coremidi]")
{
  std::atomic_int errors{};
  const auto error_hook = [&](std::string_view, const libremidi::source_location&) { ++errors; };
  {
    libremidi::midi_out_core failed{{.on_error = error_hook}, {}};
    REQUIRE(failed.is_client_open() != stdx::error{});
  }
  REQUIRE(errors == 1);
  REQUIRE(invalid_disposals == 0);
  REQUIRE(clients.empty());

  std::atomic_int open_failures{};
  std::vector<std::thread> threads;
  for (int i = 0; i < 8; ++i)
  {
    threads.emplace_back([&] {
      for (int n = 0; n < 8; ++n)
      {
        libremidi::midi_in_core input{{.on_error = error_hook}, {}};
        libremidi::midi_out_core output{{.on_error = error_hook}, {}};
        libremidi::coremidi_ump::midi_in_impl ump_input{{.on_error = error_hook}, {}};
        libremidi::coremidi_ump::midi_out_impl ump_output{{.on_error = error_hook}, {}};
        open_failures += input.is_client_open() != stdx::error{};
        open_failures += output.is_client_open() != stdx::error{};
        open_failures += ump_input.is_client_open() != stdx::error{};
        open_failures += ump_output.is_client_open() != stdx::error{};
      }
    });
  }
  for (auto& thread : threads)
    thread.join();
  REQUIRE(open_failures == 0);
  REQUIRE(keeper_attempts == 2); // One failed creation, one successful keeper.
  REQUIRE(clients.size() == 1);
  REQUIRE(clients.begin()->second);
  REQUIRE_FALSE(keeper_has_callback);
  REQUIRE_FALSE(server_stopped);

  {
    libremidi::observer_core observer{
        {.on_error = error_hook, .input_added = [](const libremidi::input_port&) {},
         .notify_in_constructor = false}, {}};
    observer.finish_init();
    REQUIRE(clients.size() == 2);
  }
  REQUIRE(clients.size() == 1);

  fail_owned = true;
  {
    libremidi::midi_out_core failed{{.on_error = error_hook}, {}};
    REQUIRE(failed.is_client_open() != stdx::error{});
  }
  {
    libremidi::midi_out_core reopened{{.on_error = error_hook}, {}};
    REQUIRE(reopened.is_client_open() == stdx::error{});
  }
  {
    libremidi::midi_in_core borrowed{{.on_error = error_hook}, {.context = 900}};
    REQUIRE(borrowed.is_client_open() == stdx::error{});
    REQUIRE(borrowed.client == 900);
  }
  REQUIRE(errors == 2);
  REQUIRE(keeper_attempts == 2);
  REQUIRE(keeper_disposals == 0);
  REQUIRE(invalid_disposals == 0);
  REQUIRE(clients.size() == 1);
}

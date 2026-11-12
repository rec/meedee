#pragma once
#include <libremidi/backends/coremidi/error_domain.hpp>
#include <CoreMIDI/CoreMIDI.h>
#include <CoreServices/CoreServices.h>

#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <limits>
#include <mutex>
#include <thread>

NAMESPACE_LIBREMIDI
{
// One retained instance owns the first client and its run loop until process exit.
// It never holds application callback pointers or client objects.
class coremidi_notifications
{
public:
  static coremidi_notifications& instance()
  {
    static auto* state = new coremidi_notifications;
    return *state;
  }

  OSStatus ensure()
  {
    std::lock_guard startup{startup_mutex};
    std::unique_lock lock{mutex};
    if (client)
      return status;
    if (thread.joinable())
    {
      lock.unlock();
      thread.join(); // Only a completed failed startup reaches this path.
      lock.lock();
    }
    ready = false;
    thread = std::thread{[this] { run(); }};
    changed.wait(lock, [this] { return ready; });
    return status;
  }

  uint64_t mark()
  {
    std::lock_guard lock{mutex};
    return sequence;
  }

  stdx::error wait(uint64_t after, MIDIObjectRef object, MIDINotificationMessageID kind)
  {
    std::unique_lock lock{mutex};
    if (std::this_thread::get_id() == owner)
      return std::errc::resource_deadlock_would_occur;
    const auto found = [&] {
      for (const auto& event : history)
        if (event.sequence > after && event.object == object && event.kind == kind)
          return true;
      return false;
    };
    const auto overflow = [&] { return sequence - after > history.size(); };
    changed.wait_until(lock, std::chrono::steady_clock::now() + std::chrono::seconds{2},
                       [&] { return status != noErr || found() || overflow(); });
    if (status != noErr)
      return from_osstatus(status);
    if (found())
      return {};
    if (overflow())
      return std::errc::no_buffer_space;
    return std::errc::timed_out;
  }

private:
  struct event
  {
    uint64_t sequence{};
    MIDIObjectRef object{};
    MIDINotificationMessageID kind{};
  };

  static void notify(const MIDINotification* message, void* context)
  {
    if ((message->messageID != kMIDIMsgObjectAdded && message->messageID != kMIDIMsgObjectRemoved)
        || message->messageSize < sizeof(MIDIObjectAddRemoveNotification))
      return;
    const auto& object = *reinterpret_cast<const MIDIObjectAddRemoveNotification*>(message);
    if (object.childType != kMIDIObjectType_Source && object.childType != kMIDIObjectType_Destination)
      return;
    auto& self = *static_cast<coremidi_notifications*>(context);
    std::lock_guard lock{self.mutex};
    if (self.sequence == std::numeric_limits<uint64_t>::max())
      self.status = kMIDIUnknownError;
    else
    {
      ++self.sequence;
      self.history[self.sequence % self.history.size()]
          = {self.sequence, object.child, message->messageID};
    }
    self.changed.notify_all();
  }

  void run()
  {
    CFRunLoopSourceContext context{};
    context.perform = +[](void*) {};
    const auto source = CFRunLoopSourceCreate(nullptr, 0, &context);
    MIDIClientRef candidate{};
    const auto result = source
                            ? MIDIClientCreate(CFSTR("libremidi client keeper"), notify, this, &candidate)
                            : memFullErr;
    if (result == noErr)
      CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
    {
      std::lock_guard lock{mutex};
      client = result == noErr ? candidate : 0;
      owner = std::this_thread::get_id();
      status = result;
      ready = true;
      changed.notify_all();
    }
    if (result == noErr)
    {
      CFRunLoopRun();
      std::lock_guard lock{mutex};
      status = kMIDIUnknownError; // An unexpectedly stopped loop cannot acknowledge operations.
      changed.notify_all();
    }
    if (source)
    {
      CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
      CFRelease(source);
    }
  }

  std::mutex startup_mutex, mutex;
  std::condition_variable changed;
  std::thread thread;
  std::thread::id owner;
  MIDIClientRef client{};
  OSStatus status{kMIDIServerStartErr};
  bool ready{};
  uint64_t sequence{};
  std::array<event, 256> history{};
};
}

// Direct SDK endpoint churn probe. No MIDI messages or hardware connections.
#include <CoreMIDI/CoreMIDI.h>

#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>

extern char** environ;

namespace
{
void require(bool condition, const char* operation)
{
  if (!condition)
  {
    std::fprintf(stderr, "probe failed: %s\n", operation);
    std::exit(2);
  }
}

struct Reply
{
  OSStatus status{};
  MIDIEndpointRef endpoint{};
  SInt32 unique_id{};
};

struct Change
{
  MIDINotificationMessageID kind{};
  MIDIObjectRef object{};
};

struct Probe
{
  bool producer;
  int incoming;
  int outgoing;
  MIDIClientRef keeper{}, client{};
  MIDIEndpointRef endpoint{};
  CFFileDescriptorRef descriptor{};
  CFRunLoopSourceRef source{};
  CFRunLoopTimerRef timer{};
  std::array<Change, 128> changes{};
  std::size_t change_count{};
  char command{};
  Reply reply{};
  MIDIEndpointRef expected{};
  int cycle{1};
  bool awaiting{}, ready{}, busy{}, valid{true};

  FILE* log() const { return producer ? stderr : stdout; }
  const char* role() const { return producer ? "producer" : "consumer"; }

  void cancel_deadline()
  {
    if (timer)
    {
      CFRunLoopTimerInvalidate(timer);
      CFRelease(timer);
      timer = nullptr;
    }
  }

  void begin(char next)
  {
    command = next;
    change_count = 0;
    ready = false;
    awaiting = true;
    expected = next == 'x' ? endpoint : 0;
    CFRunLoopTimerContext context{0, this, nullptr, nullptr, nullptr};
    timer = CFRunLoopTimerCreate(
        nullptr, CFAbsoluteTimeGetCurrent() + 2, 0, 0, 0,
        +[](CFRunLoopTimerRef, void* info) {
          const auto& state = *static_cast<Probe*>(info);
          std::fprintf(stderr,
                       "%s notification deadline: cycle=%d command=%c expected=%u reply=%d\n",
                       state.role(), state.cycle, state.command, state.expected, state.ready);
          std::exit(3);
        }, &context);
    require(timer != nullptr, "create notification deadline");
    CFRunLoopAddTimer(CFRunLoopGetCurrent(), timer, kCFRunLoopDefaultMode);
    std::fprintf(log(), "%s cycle=%d command=%c waiting\n", role(), cycle, command);
    if (!producer)
      require(write(outgoing, &command, 1) == 1, "write producer command");
  }

  bool enumerate(bool present)
  {
    const auto count = MIDIGetNumberOfDestinations();
    int matches{};
    bool complete = true;
    for (ItemCount index = 0; index < count; ++index)
    {
      const auto object = MIDIGetDestination(index);
      SInt32 unique_id{};
      const auto status = MIDIObjectGetIntegerProperty(object, kMIDIPropertyUniqueID, &unique_id);
      std::printf("index=%lu endpoint=%u status=%d unique_id=%d\n",
                  static_cast<unsigned long>(index), object, status, unique_id);
      complete &= status == noErr;
      matches += status == noErr && unique_id == reply.unique_id;
    }
    std::printf("cycle=%d expected_present=%d test-owned matches=%d\n", cycle, present, matches);
    return complete && matches == (present ? 1 : 0);
  }

  void advance()
  {
    if (!awaiting || !ready || busy)
      return;
    const auto kind = command == 'c' ? kMIDIMsgObjectAdded : kMIDIMsgObjectRemoved;
    bool observed = false;
    for (std::size_t index = 0; index < change_count; ++index)
      observed |= changes[index].kind == kind && changes[index].object == expected;
    if (!observed)
      return;
    awaiting = false;
    cancel_deadline();
    std::fprintf(log(), "%s cycle=%d matched notification=%d endpoint=%u\n",
                 role(), cycle, kind, expected);
    if (producer)
    {
      require(write(outgoing, &reply, sizeof(reply)) == sizeof(reply), "write producer reply");
      if (command == 'x')
        ++cycle;
      return;
    }
    valid &= enumerate(command == 'c');
    if (command == 'c')
    {
      MIDIPortRef port{};
      require(MIDIClientCreate(CFSTR("meedee-churn-output"), nullptr, nullptr, &client) == noErr,
              "create consumer client");
      require(MIDIOutputPortCreate(client, CFSTR("meedee-churn-output"), &port) == noErr,
              "create consumer port");
      require(MIDIPortDispose(port) == noErr, "dispose consumer port");
      require(MIDIClientDispose(client) == noErr, "dispose consumer client");
      client = 0;
      begin('x');
    }
    else if (++cycle <= 3)
      begin('c');
    else
    {
      close(outgoing); // EOF wakes the producer's pipe source so its run loop can stop.
      outgoing = -1;
      CFRunLoopStop(CFRunLoopGetCurrent());
    }
  }

  static void notification(const MIDINotification* message, void* info)
  {
    auto& state = *static_cast<Probe*>(info);
    std::fprintf(state.log(), "%s notification=%d\n", state.role(), message->messageID);
    if (!state.awaiting || (message->messageID != kMIDIMsgObjectAdded
                           && message->messageID != kMIDIMsgObjectRemoved))
      return;
    require(message->messageSize >= sizeof(MIDIObjectAddRemoveNotification), "notification size");
    const auto& change = *reinterpret_cast<const MIDIObjectAddRemoveNotification*>(message);
    if (change.childType != kMIDIObjectType_Destination)
      return;
    require(state.change_count < state.changes.size(), "notification buffer capacity");
    state.changes[state.change_count++] = {message->messageID, change.child};
    std::fprintf(state.log(), "%s destination change=%d endpoint=%u\n",
                 state.role(), message->messageID, change.child);
    state.advance();
  }

  void produce()
  {
    char next{};
    const auto size = read(incoming, &next, 1); // Called only when the pipe is readable.
    if (size == 0)
    {
      CFRunLoopStop(CFRunLoopGetCurrent());
      return;
    }
    require(size == 1 && !awaiting, "read next producer command");
    begin(next);
    busy = true;
    reply = {};
    if (command == 'c')
    {
      reply.status = MIDIClientCreate(CFSTR("meedee-churn-input"), nullptr, nullptr, &client);
      if (reply.status == noErr)
        reply.status = MIDIDestinationCreate(
            client, CFSTR("meedee-churn-test-owned"),
            +[](const MIDIPacketList*, void*, void*) {}, nullptr, &endpoint);
      expected = endpoint;
      reply.endpoint = endpoint;
      if (reply.status == noErr)
        reply.status = MIDIObjectGetIntegerProperty(endpoint, kMIDIPropertyUniqueID, &reply.unique_id);
    }
    else
    {
      require(command == 'x', "known producer command");
      reply.status = MIDIEndpointDispose(endpoint);
      if (reply.status == noErr)
      {
        endpoint = 0;
        reply.status = MIDIClientDispose(client);
        if (reply.status == noErr)
          client = 0;
      }
    }
    std::fprintf(stderr, "producer SDK status=%d endpoint=%u unique_id=%d\n",
                 reply.status, reply.endpoint, reply.unique_id);
    require(reply.status == noErr, "producer SDK operation");
    ready = true;
    busy = false;
    advance();
  }

  void consume()
  {
    Reply received;
    require(read(incoming, &received, sizeof(received)) == sizeof(received), "read producer reply");
    require(awaiting && received.status == noErr, "successful pending producer reply");
    if (command == 'c')
    {
      reply = received;
      endpoint = received.endpoint;
      expected = endpoint;
    }
    ready = true;
    advance();
  }

  void run()
  {
    const auto status = MIDIClientCreate(CFSTR("meedee-churn-keeper"), notification, this, &keeper);
    std::fprintf(log(), "%s MIDIClientCreate=%d keeper=%u\n", role(), status, keeper);
    require(status == noErr, "create notification client");
    CFFileDescriptorContext context{0, this, nullptr, nullptr, nullptr};
    descriptor = CFFileDescriptorCreate(
        nullptr, incoming, false,
        +[](CFFileDescriptorRef descriptor, CFOptionFlags, void* info) {
          auto& state = *static_cast<Probe*>(info);
          if (state.producer)
            state.produce();
          else
            state.consume();
          CFFileDescriptorEnableCallBacks(descriptor, kCFFileDescriptorReadCallBack);
        }, &context);
    require(descriptor != nullptr, "create pipe descriptor");
    source = CFFileDescriptorCreateRunLoopSource(nullptr, descriptor, 0);
    require(source != nullptr, "create pipe run-loop source");
    CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
    CFFileDescriptorEnableCallBacks(descriptor, kCFFileDescriptorReadCallBack);
    if (!producer)
      begin('c');
    CFRunLoopRun();
    CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
    CFFileDescriptorInvalidate(descriptor);
    CFRelease(source);
    CFRelease(descriptor);
    cancel_deadline();
    // Keeper and callback context remain alive until this process exits.
  }
};
}

int main(int argc, char** argv)
{
  std::setbuf(stdout, nullptr);
  if (argc == 2 && std::strcmp(argv[1], "--producer") == 0)
  {
    Probe producer{true, STDIN_FILENO, STDOUT_FILENO};
    producer.run();
    return 0;
  }
  require(argc == 1, "unexpected probe arguments");
  int commands[2], replies[2];
  require(pipe(commands) == 0 && pipe(replies) == 0, "create pipes");
  posix_spawn_file_actions_t actions;
  require(posix_spawn_file_actions_init(&actions) == 0, "initialize spawn actions");
  require(posix_spawn_file_actions_adddup2(&actions, commands[0], STDIN_FILENO) == 0,
          "redirect producer commands");
  require(posix_spawn_file_actions_adddup2(&actions, replies[1], STDOUT_FILENO) == 0,
          "redirect producer replies");
  const int inherited[] = {commands[0], commands[1], replies[0], replies[1]};
  for (const auto descriptor : inherited)
    require(posix_spawn_file_actions_addclose(&actions, descriptor) == 0, "close inherited pipe");
  char producer_argument[] = "--producer";
  char* arguments[] = {argv[0], producer_argument, nullptr};
  pid_t child{};
  require(posix_spawn(&child, argv[0], &actions, nullptr, arguments, environ) == 0,
          "spawn fresh producer");
  posix_spawn_file_actions_destroy(&actions);
  close(commands[0]);
  close(replies[1]);
  Probe consumer{false, replies[0], commands[1]};
  consumer.run();
  close(replies[0]);
  int status{};
  require(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0,
          "producer exit");
  return consumer.valid ? 0 : 1;
}

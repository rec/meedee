// Direct SDK endpoint churn probe. No MIDI messages or hardware connections.
#include <CoreMIDI/CoreMIDI.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>

extern char** environ;

namespace
{
struct Reply
{
  OSStatus status{};
  MIDIEndpointRef endpoint{};
  SInt32 unique_id{};
};

void notification(const MIDINotification* message, void*)
{
  std::printf("notification=%d\n", message->messageID);
}

void require(bool condition, const char* operation)
{
  if (!condition)
  {
    std::fprintf(stderr, "probe failed: %s\n", operation);
    std::exit(2);
  }
}

void producer(int commands, int replies)
{
  MIDIClientRef keeper{}, client{};
  MIDIEndpointRef endpoint{};
  char command{};
  while (read(commands, &command, 1) == 1)
  {
    Reply reply;
    if (command == 'c')
    {
      if (!keeper)
        reply.status = MIDIClientCreate(CFSTR("meedee-churn-keeper"), nullptr, nullptr, &keeper);
      if (reply.status == noErr)
        reply.status = MIDIClientCreate(CFSTR("meedee-churn-input"), nullptr, nullptr, &client);
      if (reply.status == noErr)
        reply.status = MIDIDestinationCreate(
            client, CFSTR("meedee-churn-test-owned"),
            +[](const MIDIPacketList*, void*, void*) {}, nullptr, &endpoint);
      reply.endpoint = endpoint;
      if (reply.status == noErr)
        reply.status = MIDIObjectGetIntegerProperty(endpoint, kMIDIPropertyUniqueID, &reply.unique_id);
    }
    else if (command == 'x')
    {
      reply.status = MIDIEndpointDispose(endpoint);
      if (reply.status == noErr)
      {
        endpoint = 0;
        reply.status = MIDIClientDispose(client);
        if (reply.status == noErr)
          client = 0;
      }
    }
    else
      std::exit(2);
    require(write(replies, &reply, sizeof(reply)) == sizeof(reply), "write producer reply");
  }
  // The private keeper is left to process cleanup, matching the library contract.
  std::exit(0);
}

Reply exchange(int commands, int replies, char command)
{
  require(write(commands, &command, 1) == 1, "write producer command");
  Reply reply;
  require(read(replies, &reply, sizeof(reply)) == sizeof(reply), "read producer reply");
  std::printf("producer command=%c status=%d endpoint=%u unique_id=%d\n",
              command, reply.status, reply.endpoint, reply.unique_id);
  require(reply.status == noErr, "producer SDK operation");
  return reply;
}
}

int main(int argc, char** argv)
{
  if (argc == 2 && std::strcmp(argv[1], "--producer") == 0)
    producer(STDIN_FILENO, STDOUT_FILENO);
  require(argc == 2, "expected a discovery probe mode");
  const bool notified = std::strcmp(argv[1], "notified") == 0
                        || std::strcmp(argv[1], "notified_serviced") == 0;
  const bool early = notified || std::strcmp(argv[1], "early") == 0;
  const bool serviced = std::strcmp(argv[1], "serviced") == 0
                        || std::strcmp(argv[1], "notified_serviced") == 0;
  require(early || serviced || std::strcmp(argv[1], "late") == 0, "unknown mode");
  std::setbuf(stdout, nullptr);
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
  MIDIClientRef keeper{};
  if (early)
    require(MIDIClientCreate(
                CFSTR("meedee-churn-keeper"), notified ? notification : nullptr, nullptr, &keeper)
                == noErr,
            "create early consumer keeper");
  bool valid = true;
  for (int cycle = 1; cycle <= 3; ++cycle)
  {
    std::printf("mode=%s cycle=%d\n", argv[1], cycle);
    const auto created = exchange(commands[1], replies[0], 'c');
    const auto runloop = CFRunLoopRunInMode(kCFRunLoopDefaultMode, serviced ? 0.05 : 0, false);
    const auto count = MIDIGetNumberOfDestinations();
    std::printf("runloop=%d destinations=%lu\n", runloop, static_cast<unsigned long>(count));
    int matches{};
    for (ItemCount index = 0; index < count; ++index)
    {
      const auto endpoint = MIDIGetDestination(index);
      SInt32 unique_id{};
      const auto status = MIDIObjectGetIntegerProperty(endpoint, kMIDIPropertyUniqueID, &unique_id);
      std::printf("index=%lu endpoint=%u status=%d unique_id=%d\n",
                  static_cast<unsigned long>(index), endpoint, status, unique_id);
      valid &= status == noErr;
      matches += status == noErr && unique_id == created.unique_id;
    }
    valid &= matches == 1;
    std::printf("test-owned matches=%d\n", matches);
    if (!keeper)
      require(MIDIClientCreate(CFSTR("meedee-churn-keeper"), nullptr, nullptr, &keeper) == noErr,
              "create late consumer keeper");
    MIDIClientRef client{};
    MIDIPortRef port{};
    require(MIDIClientCreate(CFSTR("meedee-churn-output"), nullptr, nullptr, &client) == noErr,
            "create consumer client");
    require(MIDIOutputPortCreate(client, CFSTR("meedee-churn-output"), &port) == noErr,
            "create consumer port");
    require(MIDIPortDispose(port) == noErr, "dispose consumer port");
    require(MIDIClientDispose(client) == noErr, "dispose consumer client");
    exchange(commands[1], replies[0], 'x');
  }
  close(commands[1]);
  close(replies[0]);
  int status{};
  require(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0,
          "producer exit");
  return valid ? 0 : 1;
}

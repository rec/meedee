// Opt-in direct access probe. Creates only its own client; no endpoints or sends.
#include <CoreMIDI/CoreMIDI.h>
#include <cstdio>

int main()
{
  MIDIClientRef client{};
  const OSStatus created = MIDIClientCreate(
      CFSTR("meedee-access-probe"), nullptr, nullptr, &client);
  std::printf("MIDIClientCreate: %d (0x%08x), client=%u\n",
              created, static_cast<unsigned>(created), client);
  if (created != noErr)
    return 1;
  const OSStatus disposed = MIDIClientDispose(client);
  std::printf("MIDIClientDispose: %d\n", disposed);
  return disposed == noErr ? 0 : 2;
}

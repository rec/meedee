// Test executable symbol substitution, like the fork's property regression
// test. Pure CoreMIDI packet/CF helpers remain real; no MIDI client reaches the
// OS.
#include <Block.h>
#include <CoreMIDI/CoreMIDI.h>
#include <atomic>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <vector>

// CoreMIDI normally initializes these through its client setup. The fixture
// supplies distinct property keys without contacting the daemon.
extern "C" const CFStringRef kMIDIPropertyUniqueID = CFSTR("fixture-uniqueID");
extern "C" const CFStringRef kMIDIPropertyName = CFSTR("fixture-name");
extern "C" const CFStringRef kMIDIPropertyManufacturer =
    CFSTR("fixture-manufacturer");
extern "C" const CFStringRef kMIDIPropertyModel = CFSTR("fixture-model");
extern "C" const CFStringRef kMIDIPropertyDisplayName =
    CFSTR("fixture-displayName");
extern "C" const CFStringRef kMIDIPropertyProtocolID =
    CFSTR("fixture-protocolID");

namespace {
struct endpoint {
  MIDIClientRef client{};
  bool source{};
  CFStringRef name{};
  MIDIProtocolID protocol{kMIDIProtocol_1_0};
  MIDIReadProc read{};
  void *context{};
  MIDIReceiveBlock block{};
  bool fail_disposal{};
  bool retired{};
  int sends_remaining{-1};
  MIDIEndpointRef connected_source{};
  std::mutex delivery;
  ~endpoint() {
    if (name)
      CFRelease(name);
    if (block)
      Block_release(block);
  }
};
struct state {
  std::mutex mutex;
  std::unordered_map<MIDIEndpointRef, std::shared_ptr<endpoint>> endpoints;
  std::unordered_map<MIDIPortRef, std::shared_ptr<endpoint>> inputs;
  std::atomic<UInt32> next{100};
  std::atomic_bool enabled{};
};
state &registry() {
  static state instance;
  return instance;
}

std::shared_ptr<endpoint> find(MIDIEndpointRef id) {
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  auto found = state.endpoints.find(id);
  return found == state.endpoints.end() ? nullptr : found->second;
}
OSStatus create(MIDIClientRef client, CFStringRef name, bool source,
                MIDIReadProc read, void *context, MIDIProtocolID protocol,
                MIDIReceiveBlock block, MIDIEndpointRef *out) {
  if (!registry().enabled || !name)
    return kMIDIInvalidClient;
  auto value = std::make_shared<endpoint>();
  value->client = client;
  value->source = source;
  value->name = name;
  CFRetain(name);
  value->read = read;
  value->context = context;
  value->protocol = protocol;
  if (block)
    value->block = Block_copy(block);
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  *out = state.next++;
  state.endpoints.emplace(*out, std::move(value));
  return noErr;
}
MIDIEndpointRef at(bool source, ItemCount index) {
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  for (const auto &[id, endpoint] : state.endpoints)
    if (endpoint->source == source && index-- == 0)
      return id;
  return 0;
}
ItemCount count(bool source) {
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  ItemCount result{};
  for (const auto &[_, endpoint] : state.endpoints)
    if (endpoint->source == source)
      ++result;
  return result;
}
} // namespace

extern "C" void md_fixture_enable() { registry().enabled = true; }
extern "C" void md_fixture_fail_disposal(UInt32 id) {
  if (auto endpoint = find(id)) {
    std::lock_guard guard{endpoint->delivery};
    endpoint->fail_disposal = true;
  }
}
extern "C" void md_fixture_fail_send_after(UInt32 id, int accepted) {
  if (auto endpoint = find(id)) {
    std::lock_guard guard{endpoint->delivery};
    endpoint->sends_remaining = accepted;
  }
}
extern "C" bool md_fixture_endpoint_exists(UInt32 id) { return bool(find(id)); }

extern "C" OSStatus MIDIClientCreate(CFStringRef, MIDINotifyProc, void *,
                                     MIDIClientRef *out) {
  if (!registry().enabled)
    return kMIDIInvalidClient;
  *out = registry().next++;
  return noErr;
}
extern "C" OSStatus MIDIClientDispose(MIDIClientRef) { return noErr; }
extern "C" ItemCount MIDIGetNumberOfSources() { return count(true); }
extern "C" ItemCount MIDIGetNumberOfDestinations() { return count(false); }
extern "C" MIDIEndpointRef MIDIGetSource(ItemCount index) {
  return at(true, index);
}
extern "C" MIDIEndpointRef MIDIGetDestination(ItemCount index) {
  return at(false, index);
}
extern "C" OSStatus MIDIEndpointGetEntity(MIDIEndpointRef,
                                          MIDIEntityRef *entity) {
  *entity = 0;
  return noErr;
}
extern "C" OSStatus MIDIEntityGetDevice(MIDIEntityRef, MIDIDeviceRef *device) {
  *device = 0;
  return kMIDIObjectNotFound;
}
extern "C" OSStatus MIDIObjectGetIntegerProperty(MIDIObjectRef object,
                                                 CFStringRef property,
                                                 SInt32 *out) {
  auto endpoint = find(object);
  if (!endpoint)
    return kMIDIObjectNotFound;
  if (CFEqual(property, kMIDIPropertyUniqueID))
    *out = static_cast<SInt32>(object);
  else if (CFEqual(property, kMIDIPropertyProtocolID))
    *out = endpoint->protocol;
  else
    return kMIDIUnknownProperty;
  return noErr;
}
extern "C" OSStatus MIDIObjectGetStringProperty(MIDIObjectRef object,
                                                CFStringRef property,
                                                CFStringRef *out) {
  auto endpoint = find(object);
  if (!endpoint)
    return kMIDIObjectNotFound;
  *out = (CFEqual(property, kMIDIPropertyName) ||
          CFEqual(property, kMIDIPropertyDisplayName))
             ? endpoint->name
             : CFSTR("");
  CFRetain(*out);
  return noErr;
}
extern "C" OSStatus MIDIObjectFindByUniqueID(MIDIUniqueID id,
                                             MIDIObjectRef *out,
                                             MIDIObjectType *type) {
  auto endpoint = find(id);
  if (!endpoint)
    return kMIDIObjectNotFound;
  *out = id;
  *type =
      endpoint->source ? kMIDIObjectType_Source : kMIDIObjectType_Destination;
  return noErr;
}
extern "C" OSStatus MIDIDestinationCreate(MIDIClientRef client,
                                          CFStringRef name, MIDIReadProc read,
                                          void *ctx, MIDIEndpointRef *out) {
  return create(client, name, false, read, ctx, kMIDIProtocol_1_0, nullptr,
                out);
}
extern "C" OSStatus MIDIDestinationCreateWithProtocol(MIDIClientRef client,
                                                      CFStringRef name,
                                                      MIDIProtocolID protocol,
                                                      MIDIEndpointRef *out,
                                                      MIDIReceiveBlock block) {
  return create(client, name, false, nullptr, nullptr, protocol, block, out);
}
extern "C" OSStatus MIDISourceCreate(MIDIClientRef client, CFStringRef name,
                                     MIDIEndpointRef *out) {
  return create(client, name, true, nullptr, nullptr, kMIDIProtocol_1_0,
                nullptr, out);
}
extern "C" OSStatus MIDISourceCreateWithProtocol(MIDIClientRef client,
                                                 CFStringRef name,
                                                 MIDIProtocolID protocol,
                                                 MIDIEndpointRef *out) {
  return create(client, name, true, nullptr, nullptr, protocol, nullptr, out);
}
extern "C" OSStatus MIDIEndpointDispose(MIDIEndpointRef id) {
  auto endpoint = find(id);
  if (!endpoint)
    return kMIDIUnknownEndpoint;
  std::lock_guard delivery{endpoint->delivery};
  if (endpoint->fail_disposal)
    return kMIDIUnknownError;
  endpoint->retired = true;
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  state.endpoints.erase(id);
  return noErr;
}
extern "C" OSStatus MIDIOutputPortCreate(MIDIClientRef, CFStringRef,
                                         MIDIPortRef *out) {
  *out = registry().next++;
  return noErr;
}
extern "C" OSStatus MIDIInputPortCreate(MIDIClientRef client, CFStringRef name,
                                        MIDIReadProc read, void *ctx,
                                        MIDIPortRef *out) {
  auto value = std::make_shared<endpoint>();
  value->client = client;
  value->name = name;
  CFRetain(name);
  value->read = read;
  value->context = ctx;
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  *out = state.next++;
  state.inputs.emplace(*out, std::move(value));
  return noErr;
}
extern "C" OSStatus MIDIInputPortCreateWithProtocol(MIDIClientRef client,
                                                    CFStringRef name,
                                                    MIDIProtocolID protocol,
                                                    MIDIPortRef *out,
                                                    MIDIReceiveBlock block) {
  auto result = MIDIInputPortCreate(client, name, nullptr, nullptr, out);
  if (result)
    return result;
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  auto &value = state.inputs.at(*out);
  value->protocol = protocol;
  value->block = Block_copy(block);
  return noErr;
}
extern "C" OSStatus MIDIPortConnectSource(MIDIPortRef port,
                                          MIDIEndpointRef source, void *) {
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  auto found = state.inputs.find(port);
  if (found == state.inputs.end())
    return kMIDIInvalidPort;
  found->second->connected_source = source;
  return noErr;
}
extern "C" OSStatus MIDIPortDisconnectSource(MIDIPortRef port,
                                             MIDIEndpointRef) {
  auto &state = registry();
  std::lock_guard guard{state.mutex};
  auto found = state.inputs.find(port);
  if (found == state.inputs.end())
    return kMIDIInvalidPort;
  found->second->connected_source = 0;
  return noErr;
}
extern "C" OSStatus MIDIPortDispose(MIDIPortRef port) {
  std::shared_ptr<endpoint> value;
  {
    auto &state = registry();
    std::lock_guard guard{state.mutex};
    auto found = state.inputs.find(port);
    if (found != state.inputs.end()) {
      value = found->second;
      state.inputs.erase(found);
    }
  }
  if (value) {
    std::lock_guard drain{value->delivery};
    value->retired = true;
  }
  return noErr;
}
extern "C" OSStatus MIDISend(MIDIPortRef, MIDIEndpointRef id,
                             const MIDIPacketList *packets) {
  auto endpoint = find(id);
  if (!endpoint)
    return kMIDIUnknownEndpoint;
  std::lock_guard guard{endpoint->delivery};
  if (endpoint->retired)
    return kMIDIUnknownEndpoint;
  if (endpoint->sends_remaining == 0)
    return kMIDIMessageSendErr;
  if (endpoint->sends_remaining > 0)
    --endpoint->sends_remaining;
  if (endpoint->read)
    endpoint->read(packets, endpoint->context, nullptr);
  return noErr;
}
extern "C" OSStatus MIDISendEventList(MIDIPortRef, MIDIEndpointRef id,
                                      const MIDIEventList *packets) {
  auto endpoint = find(id);
  if (!endpoint)
    return kMIDIUnknownEndpoint;
  std::lock_guard guard{endpoint->delivery};
  if (endpoint->retired)
    return kMIDIUnknownEndpoint;
  if (endpoint->sends_remaining == 0)
    return kMIDIMessageSendErr;
  if (endpoint->sends_remaining > 0)
    --endpoint->sends_remaining;
  if (endpoint->block)
    endpoint->block(packets, nullptr);
  return noErr;
}
extern "C" int md_fixture_emit(UInt32 id, const UInt8 *bytes, size_t size) {
  std::vector<UInt8> storage(size + sizeof(MIDIPacketList) + 16);
  auto *list = reinterpret_cast<MIDIPacketList *>(storage.data());
  auto *packet = MIDIPacketListInit(list);
  if (!MIDIPacketListAdd(list, storage.size(), packet, 1, size, bytes))
    return kMIDIUnknownError;
  return MIDISend(0, id, list);
}

extern "C" OSStatus MIDIReceived(MIDIEndpointRef source,
                                 const MIDIPacketList *packets) {
  std::vector<std::shared_ptr<endpoint>> recipients;
  {
    auto &state = registry();
    std::lock_guard guard{state.mutex};
    for (auto &[_, value] : state.inputs)
      if (value->connected_source == source)
        recipients.push_back(value);
  }
  for (auto &value : recipients) {
    std::lock_guard guard{value->delivery};
    if (!value->retired && value->read)
      value->read(packets, value->context, nullptr);
  }
  return noErr;
}
extern "C" OSStatus MIDIReceivedEventList(MIDIEndpointRef source,
                                          const MIDIEventList *packets) {
  std::vector<std::shared_ptr<endpoint>> recipients;
  {
    auto &state = registry();
    std::lock_guard guard{state.mutex};
    for (auto &[_, value] : state.inputs)
      if (value->connected_source == source)
        recipients.push_back(value);
  }
  for (auto &value : recipients) {
    std::lock_guard guard{value->delivery};
    if (!value->retired && value->block)
      value->block(packets, nullptr);
  }
  return noErr;
}

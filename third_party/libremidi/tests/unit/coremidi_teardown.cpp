#include "../include_catch.hpp"
#include <libremidi/backends/coremidi/helpers.hpp>
#include <libremidi/libremidi-c.h>
#include <libremidi/port_information.hpp>

#include <cerrno>

namespace
{
OSStatus endpoint_status = noErr;
OSStatus port_status = noErr;
OSStatus connect_status = noErr;
OSStatus output_create_status = noErr;
MIDIObjectType object_type = kMIDIObjectType_Source;
int endpoint_disposals{};
int port_disposals{};
int client_disposals{};
MIDIReadProc read_callback{};
void* read_context{};

void reset()
{
  endpoint_status = port_status = connect_status = noErr;
  output_create_status = noErr;
  object_type = kMIDIObjectType_Source;
  endpoint_disposals = port_disposals = client_disposals = 0;
  read_callback = nullptr;
  read_context = nullptr;
}

libremidi_midi_configuration input_configuration(int& calls)
{
  libremidi_midi_configuration conf{};
  libremidi_midi_configuration_init(&conf);
  conf.version = libremidi_midi_configuration::MIDI1;
  conf.virtual_port = true;
  conf.port_name = "checked teardown";
  conf.ignore_timing = conf.ignore_sensing = conf.ignore_sysex = false;
  conf.on_midi1_message.context = &calls;
  conf.on_midi1_message.callback = [](void* ctx, int64_t, const uint8_t*, size_t) {
    ++*static_cast<int*>(ctx);
  };
  return conf;
}

void deliver()
{
  MIDIPacketList list{};
  auto* packet = MIDIPacketListInit(&list);
  const Byte bytes[]{0x90, 60, 100};
  REQUIRE(MIDIPacketListAdd(&list, sizeof(list), packet, 1, sizeof(bytes), bytes));
  REQUIRE(read_callback);
  read_callback(&list, read_context, nullptr);
}
}

// Fake CoreMIDI resources only: never create clients or ports in the OS.
extern "C" OSStatus MIDIClientCreate(CFStringRef, MIDINotifyProc, void*, MIDIClientRef* out)
{
  *out = 10;
  return noErr;
}
extern "C" OSStatus MIDIClientDispose(MIDIClientRef)
{
  ++client_disposals;
  return noErr;
}
extern "C" OSStatus MIDIDestinationCreate(
    MIDIClientRef, CFStringRef, MIDIReadProc callback, void* ctx, MIDIEndpointRef* out)
{
  read_callback = callback;
  read_context = ctx;
  *out = 20;
  return noErr;
}
extern "C" OSStatus MIDIInputPortCreate(
    MIDIClientRef, CFStringRef, MIDIReadProc callback, void* ctx, MIDIPortRef* out)
{
  read_callback = callback;
  read_context = ctx;
  *out = 30;
  return noErr;
}
extern "C" OSStatus MIDIOutputPortCreate(MIDIClientRef, CFStringRef, MIDIPortRef* out)
{
  if (output_create_status != noErr)
    return output_create_status;
  *out = 30;
  return noErr;
}
extern "C" OSStatus MIDISourceCreate(MIDIClientRef, CFStringRef, MIDIEndpointRef* out)
{
  *out = 20;
  return noErr;
}
extern "C" OSStatus MIDISourceCreateWithProtocol(
    MIDIClientRef, CFStringRef, MIDIProtocolID, MIDIEndpointRef* out)
{
  *out = 20;
  return noErr;
}
extern "C" OSStatus MIDIObjectFindByUniqueID(
    MIDIUniqueID, MIDIObjectRef* object, MIDIObjectType* type)
{
  *object = 40;
  *type = object_type;
  return noErr;
}
extern "C" OSStatus MIDIPortConnectSource(MIDIPortRef, MIDIEndpointRef, void*)
{
  return connect_status;
}
extern "C" OSStatus MIDIEndpointDispose(MIDIEndpointRef)
{
  ++endpoint_disposals;
  return endpoint_status;
}
extern "C" OSStatus MIDIPortDispose(MIDIPortRef)
{
  ++port_disposals;
  return port_status;
}

TEST_CASE("CoreMIDI disposal retains failed handles and clears successful ones", "[coremidi]")
{
  reset();
  libremidi::coremidi_data data;
  data.endpoint = 20;
  data.port = 30;
  endpoint_status = kMIDIUnknownError;
  REQUIRE(data.close_port() != stdx::error{});
  REQUIRE(data.endpoint == 20);
  REQUIRE(data.port == 30);
  REQUIRE(port_disposals == 0);

  endpoint_status = noErr;
  port_status = kMIDIUnknownError;
  REQUIRE(data.close_port() != stdx::error{});
  REQUIRE(data.endpoint == 0);
  REQUIRE(data.port == 30);
  REQUIRE(endpoint_disposals == 2);

  port_status = noErr;
  REQUIRE(data.close_port() == stdx::error{});
  REQUIRE(data.port == 0);
  REQUIRE(data.close_port() == stdx::error{});
  REQUIRE(endpoint_disposals == 2);
  REQUIRE(port_disposals == 2);
}

TEST_CASE("C input free preserves callbacks until successful disposal", "[coremidi]")
{
  reset();
  int calls{};
  auto conf = input_configuration(calls);
  libremidi_api_configuration api{};
  libremidi_midi_api_configuration_init(&api);
  api.api = libremidi_api::COREMIDI;
  libremidi_midi_in_handle* input{};
  REQUIRE(libremidi_midi_in_new(&conf, &api, &input) == 0);
  REQUIRE(input);
  endpoint_status = kMIDIUnknownError;
  REQUIRE(libremidi_midi_in_free(input) == -EIO);
  REQUIRE(client_disposals == 0);
  deliver();
  REQUIRE(calls == 1);
  // Failure must not clear the public open flag or skip the next disposal.
  REQUIRE(libremidi_midi_in_free(input) == -EIO);
  REQUIRE(endpoint_disposals == 2);
  endpoint_status = noErr;
  REQUIRE(libremidi_midi_in_free(input) == 0);
  REQUIRE(endpoint_disposals == 3);
  REQUIRE(client_disposals == 1);
  REQUIRE(libremidi_midi_in_free(nullptr) == 0);
}

TEST_CASE("C failed input open retains the callback when rollback fails", "[coremidi]")
{
  reset();
  int calls{};
  auto conf = input_configuration(calls);
  libremidi::input_port port;
  port.api = libremidi::API::COREMIDI;
  port.port = 40;
  conf.virtual_port = false;
  conf.in_port = reinterpret_cast<libremidi_midi_in_port*>(&port);
  libremidi_api_configuration api{};
  libremidi_midi_api_configuration_init(&api);
  api.api = libremidi_api::COREMIDI;
  libremidi_midi_in_handle* input{};
  connect_status = port_status = kMIDIUnknownError;
  REQUIRE(libremidi_midi_in_new(&conf, &api, &input) == -EIO);
  REQUIRE(input);
  REQUIRE(port_disposals == 1);
  REQUIRE(client_disposals == 0);
  deliver();
  REQUIRE(calls == 1);
  port_status = noErr;
  REQUIRE(libremidi_midi_in_free(input) == 0);
  REQUIRE(port_disposals == 2);
  REQUIRE(client_disposals == 1);

  reset();
  connect_status = kMIDIUnknownError;
  REQUIRE(libremidi_midi_in_new(&conf, &api, &input) == -EIO);
  REQUIRE(input == nullptr);
  REQUIRE(port_disposals == 1);
  REQUIRE(client_disposals == 1);
}

TEST_CASE("C output free retains failed CoreMIDI byte and UMP endpoints", "[coremidi]")
{
  for (auto backend : {libremidi_api::COREMIDI, libremidi_api::COREMIDI_UMP})
  {
    reset();
    int calls{};
    auto conf = input_configuration(calls);
    libremidi_api_configuration api{};
    libremidi_midi_api_configuration_init(&api);
    api.api = backend;
    libremidi_midi_out_handle* output{};
    REQUIRE(libremidi_midi_out_new(&conf, &api, &output) == 0);
    REQUIRE(output);
    endpoint_status = kMIDIUnknownError;
    REQUIRE(libremidi_midi_out_free(output) == -EIO);
    REQUIRE(client_disposals == 0);
    REQUIRE(libremidi_midi_out_free(output) == -EIO);
    REQUIRE(endpoint_disposals == 2);
    endpoint_status = noErr;
    REQUIRE(libremidi_midi_out_free(output) == 0);
    REQUIRE(endpoint_disposals == 3);
    REQUIRE(client_disposals == 1);
  }
  REQUIRE(libremidi_midi_out_free(nullptr) == 0);
}

TEST_CASE("C output port disposal and failed-open rollback remain checked", "[coremidi]")
{
  for (auto backend : {libremidi_api::COREMIDI, libremidi_api::COREMIDI_UMP})
  {
    reset();
    object_type = kMIDIObjectType_Destination;
    libremidi::output_port port;
    port.api = static_cast<libremidi::API>(backend);
    port.port = 40;
    int calls{};
    auto conf = input_configuration(calls);
    conf.virtual_port = false;
    conf.out_port = reinterpret_cast<libremidi_midi_out_port*>(&port);
    libremidi_api_configuration api{};
    libremidi_midi_api_configuration_init(&api);
    api.api = backend;
    libremidi_midi_out_handle* output{};
    REQUIRE(libremidi_midi_out_new(&conf, &api, &output) == 0);
    REQUIRE(libremidi_midi_out_is_connected(output) == 1);
    port_status = kMIDIUnknownError;
    REQUIRE(libremidi_midi_out_free(output) == -EIO);
    REQUIRE(libremidi_midi_out_is_connected(output) == 1);
    REQUIRE(client_disposals == 0);
    port_status = noErr;
    REQUIRE(libremidi_midi_out_free(output) == 0);
    REQUIRE(port_disposals == 2);
    REQUIRE(client_disposals == 1);

    reset();
    object_type = kMIDIObjectType_Destination;
    output_create_status = kMIDIUnknownError;
    REQUIRE(libremidi_midi_out_new(&conf, &api, &output) == -EIO);
    REQUIRE(output == nullptr);
    REQUIRE(port_disposals == 0);
    REQUIRE(client_disposals == 1);
  }
}

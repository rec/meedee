#include "../include_catch.hpp"
#include <libremidi/backends/coremidi/observer.hpp>

#include <limits>

// This executable supplies the property getter, leaving its output untouched on
// failure. No MIDI clients or endpoints are created and no device is queried.
namespace
{
OSStatus property_status = noErr;
SInt32 property_value{};
CFStringRef property_string{};
}

extern "C" OSStatus
MIDIObjectGetIntegerProperty(MIDIObjectRef, CFStringRef, SInt32* out_value)
{
  if (property_status == noErr)
    *out_value = property_value;
  return property_status;
}

extern "C" OSStatus
MIDIObjectGetStringProperty(MIDIObjectRef, CFStringRef, CFStringRef* out_value)
{
  if (property_status == noErr)
  {
    *out_value = property_string;
    if (property_string)
      CFRetain(property_string);
  }
  return property_status;
}

TEST_CASE("CoreMIDI integer properties preserve successful values", "[coremidi]")
{
  property_status = noErr;
  for (const SInt32 value : {0, 1, -1, std::numeric_limits<SInt32>::min(),
                            std::numeric_limits<SInt32>::max()})
  {
    property_value = value;
    REQUIRE(libremidi::get_int_property(0, kMIDIPropertyUniqueID) == value);
  }
}

TEST_CASE("CoreMIDI failed property reads produce no port ID", "[coremidi]")
{
  property_status = kMIDIObjectNotFound;
  property_value = 123; // The failing getter deliberately does not write this.
  REQUIRE_FALSE(libremidi::get_int_property(0, kMIDIPropertyUniqueID).has_value());

  int errors{};
  std::string last_error;
  libremidi::observer_core observer{
      libremidi::observer_configuration{
          .on_error = [&](std::string_view text, const libremidi::source_location&) {
            ++errors;
            last_error = text;
          }},
      libremidi::coremidi_observer_configuration{}};
  REQUIRE_FALSE(observer.to_port_info<true>(0).has_value());
  REQUIRE_FALSE(observer.to_port_info<false>(0).has_value());
  REQUIRE(errors == 2);
  REQUIRE(last_error == "could not read CoreMIDI port unique ID");
}

TEST_CASE("CoreMIDI names preserve complete UTF-8 data", "[coremidi]")
{
  property_status = noErr;
  for (const std::string value : {std::string{}, std::string{"port"}, std::string(512, 'a'),
                                 std::string(256, 'a') + "\xc3\xa9\xf0\x9f\x8e\xb9",
                                 std::string{"a\0b", 3}})
  {
    const libremidi::CFString_handle string{CFStringCreateWithBytes(
        nullptr, reinterpret_cast<const UInt8*>(value.data()), value.size(),
        kCFStringEncodingUTF8, false)};
    REQUIRE(string);
    property_string = string.get();
    const auto retained = CFGetRetainCount(property_string);
    REQUIRE(libremidi::get_string_property(0, kMIDIPropertyName) == value);
    REQUIRE(CFGetRetainCount(property_string) == retained);
  }
  property_string = nullptr;
}

TEST_CASE("CoreMIDI failed name queries and conversions return no string", "[coremidi]")
{
  property_status = kMIDIObjectNotFound;
  REQUIRE(libremidi::get_string_property(0, kMIDIPropertyName).empty());

  property_status = noErr;
  property_string = nullptr;
  REQUIRE(libremidi::get_string_property(0, kMIDIPropertyName).empty());

  // An unpaired UTF-16 surrogate cannot be encoded as valid UTF-8 with lossByte=0.
  const UniChar unpaired = 0xD800;
  const libremidi::CFString_handle string{CFStringCreateWithCharacters(nullptr, &unpaired, 1)};
  REQUIRE(string);
  property_string = string.get();
  const auto retained = CFGetRetainCount(property_string);
  REQUIRE(libremidi::get_string_property(0, kMIDIPropertyName).empty());
  REQUIRE(CFGetRetainCount(property_string) == retained);
  property_string = nullptr;
}

#include "bridge.h"
#include <cerrno>
#include <libremidi/libremidi-c.h>
#include <new>
#if defined(_WIN32)
#include <objbase.h>
#endif

// All calls that can throw stay on the C++ side of this boundary. The native
// callbacks are non-throwing Rust trampolines and never propagate Rust panics.
#define MD_CATCH                                                               \
  catch (const std::bad_alloc &) {                                             \
    return -ENOMEM;                                                            \
  }                                                                            \
  catch (...) {                                                                \
    return -EIO;                                                               \
  }

extern "C" int md_thread_init() {
#if defined(_WIN32)
  return SUCCEEDED(CoInitializeEx(nullptr, COINIT_MULTITHREADED)) ? 0 : -EIO;
#else
  return 0;
#endif
}
extern "C" void md_thread_shutdown() {
#if defined(_WIN32)
  CoUninitialize();
#endif
}
extern "C" int md_available(uint32_t *mask) {
  try {
    *mask = 0;
    auto visit = [](void *ctx, libremidi_api api) {
      auto &bits = *static_cast<uint32_t *>(ctx);
      switch (api) {
      case COREMIDI:
        bits |= 1;
        break;
      case COREMIDI_UMP:
        bits |= 2;
        break;
      case ALSA_SEQ:
        bits |= 4;
        break;
      case ALSA_SEQ_UMP:
        bits |= 8;
        break;
      case ALSA_RAW:
        bits |= 16;
        break;
      case ALSA_RAW_UMP:
        bits |= 32;
        break;
      case WINDOWS_MM:
        bits |= 64;
        break;
      case WINDOWS_MIDI_SERVICES:
        bits |= 128;
        break;
      default:
        break; // Never expose dummy, automatic or excluded backends.
      }
    };
    libremidi_midi1_available_apis(mask, visit);
    libremidi_midi2_available_apis(mask, visit);
    return 0;
  }
  MD_CATCH
}
extern "C" int md_observer_new(int32_t api, void *ctx, md_error error,
                               void **out) {
  libremidi_midi_observer_handle *handle{};
  try {
    libremidi_observer_configuration config{};
    config.track_any = true;
    config.on_error = {ctx, error};
    libremidi_api_configuration backend{};
    backend.api = static_cast<libremidi_api>(api);
    const auto result = libremidi_midi_observer_new(&config, &backend, &handle);
    *out = handle;
    return result;
  } catch (...) {
    *out = handle;
    return -EIO;
  }
}
extern "C" int md_enumerate(void *handle, uint32_t input, void *ctx,
                            void (*visit)(void *, const void *)) {
  struct visitor {
    void *ctx;
    void (*call)(void *, const void *);
  } adapter{ctx, visit};
  try {
    auto *observer = static_cast<libremidi_midi_observer_handle *>(handle);
    if (input)
      return libremidi_midi_observer_enumerate_input_ports(
          observer, &adapter,
          [](void *ctx, const libremidi_midi_in_port *port) {
            auto &entry = *static_cast<visitor *>(ctx);
            entry.call(entry.ctx, port);
          });
    return libremidi_midi_observer_enumerate_output_ports(
        observer, &adapter, [](void *ctx, const libremidi_midi_out_port *port) {
          auto &entry = *static_cast<visitor *>(ctx);
          entry.call(entry.ctx, port);
        });
  }
  MD_CATCH
}
extern "C" int md_port_clone(uint32_t input, const void *port, void **out) {
  try {
    if (input) {
      libremidi_midi_in_port *copy{};
      const auto result = libremidi_midi_in_port_clone(
          static_cast<const libremidi_midi_in_port *>(port), &copy);
      *out = copy;
      return result;
    }
    libremidi_midi_out_port *copy{};
    const auto result = libremidi_midi_out_port_clone(
        static_cast<const libremidi_midi_out_port *>(port), &copy);
    *out = copy;
    return result;
  }
  MD_CATCH
}
extern "C" int md_port_info(uint32_t input, const void *port, uint64_t *id,
                            const char **name, size_t *size) {
  try {
    if (input) {
      auto *p = static_cast<const libremidi_midi_in_port *>(port);
      auto result = libremidi_midi_in_port_handle(p, id);
      return result ? result : libremidi_midi_in_port_name(p, name, size);
    }
    auto *p = static_cast<const libremidi_midi_out_port *>(port);
    auto result = libremidi_midi_out_port_handle(p, id);
    return result ? result : libremidi_midi_out_port_name(p, name, size);
  }
  MD_CATCH
}
extern "C" int md_port_free(uint32_t input, void *port) {
  try {
    return input ? libremidi_midi_in_port_free(
                       static_cast<libremidi_midi_in_port *>(port))
                 : libremidi_midi_out_port_free(
                       static_cast<libremidi_midi_out_port *>(port));
  }
  MD_CATCH
}
extern "C" int md_observer_free(void *handle) {
  try {
    return libremidi_midi_observer_free(
        static_cast<libremidi_midi_observer_handle *>(handle));
  }
  MD_CATCH
}
extern "C" int md_input_new(const md_input_config *c, void **out) {
  libremidi_midi_in_handle *handle{};
  try {
    libremidi_midi_configuration config{};
    config.version = c->flags & 1 ? libremidi_midi_configuration::MIDI2_RAW
                                  : libremidi_midi_configuration::MIDI1_RAW;
    config.in_port = const_cast<libremidi_midi_in_port *>(
        static_cast<const libremidi_midi_in_port *>(c->port));
    config.virtual_port = !c->port;
    config.port_name = c->name;
    config.timestamps = c->flags & 2 ? Absolute : NoTimestamp;
    if (c->flags & 1)
      config.on_midi2_raw_data = {c->context, c->words};
    else
      config.on_midi1_raw_data = {c->context, c->bytes};
    config.on_error = {c->error_context, c->error};
    libremidi_api_configuration backend{};
    backend.api = static_cast<libremidi_api>(c->api);
    const auto result = libremidi_midi_in_new(&config, &backend, &handle);
    *out = handle;
    return result;
  } catch (...) {
    *out = handle;
    return -EIO;
  }
}
extern "C" int md_input_free(void *handle) {
  try {
    return libremidi_midi_in_free(
        static_cast<libremidi_midi_in_handle *>(handle));
  }
  MD_CATCH
}
extern "C" int md_output_new(int32_t api, const void *port, const char *name,
                             void *ctx, md_error error, void **out) {
  libremidi_midi_out_handle *handle{};
  try {
    libremidi_midi_configuration config{};
    config.out_port = const_cast<libremidi_midi_out_port *>(
        static_cast<const libremidi_midi_out_port *>(port));
    config.virtual_port = !port;
    config.port_name = name;
    config.on_error = {ctx, error};
    libremidi_api_configuration backend{};
    backend.api = static_cast<libremidi_api>(api);
    const auto result = libremidi_midi_out_new(&config, &backend, &handle);
    *out = handle;
    return result;
  } catch (...) {
    *out = handle;
    return -EIO;
  }
}
extern "C" int md_output_free(void *handle) {
  try {
    return libremidi_midi_out_free(
        static_cast<libremidi_midi_out_handle *>(handle));
  }
  MD_CATCH
}
extern "C" int md_send_bytes(void *handle, const uint8_t *bytes, size_t size) {
  try {
    return libremidi_midi_out_send_message(
        static_cast<libremidi_midi_out_handle *>(handle), bytes, size);
  }
  MD_CATCH
}
extern "C" int md_send_words(void *handle, const uint32_t *words, size_t size) {
  try {
    return libremidi_midi_out_send_ump(
        static_cast<libremidi_midi_out_handle *>(handle), words, size);
  }
  MD_CATCH
}

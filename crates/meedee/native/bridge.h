#pragma once
#include <stddef.h>
#include <stdint.h>

// This is meedee's exception boundary around the pinned upstream C API.
extern "C" {
typedef void (*md_error)(void *, const char *, size_t, const void *);
typedef void (*md_bytes)(void *, int64_t, const uint8_t *, size_t);
typedef void (*md_words)(void *, int64_t, const uint32_t *, size_t);
struct md_input_config {
  int32_t api;
  uint32_t flags;   // 1: UMP, 2: timestamps
  const void *port; // null means a virtual port
  const char *name;
  void *context;
  md_bytes bytes;
  md_words words;
  void *error_context;
  md_error error;
};
int md_thread_init();
void md_thread_shutdown();
int md_available(uint32_t *);
int md_observer_new(int32_t, void *, md_error, void **);
int md_enumerate(void *, uint32_t, void *, void (*)(void *, const void *));
int md_port_clone(uint32_t, const void *, void **);
int md_port_info(uint32_t, const void *, uint64_t *, const char **, size_t *);
int md_port_free(uint32_t, void *);
int md_observer_free(void *);
int md_input_new(const md_input_config *, void **);
int md_input_free(void *);
int md_output_new(int32_t, const void *, const char *, void *, md_error,
                  void **);
int md_output_free(void *);
int md_send_bytes(void *, const uint8_t *, size_t);
int md_send_words(void *, const uint32_t *, size_t);
}

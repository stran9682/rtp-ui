#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum StreamType {
  Audio,
  Video,
} StreamType;

typedef void (*ReleaseCallback)(void*);

bool rust_send_frame(const uint8_t *data,
                     uintptr_t len,
                     enum StreamType stream,
                     void *context,
                     ReleaseCallback release_callback);

void run_runtime_server(bool is_host,
                        enum StreamType stream,
                        const uint8_t *host_addr,
                        uintptr_t host_addr_len);

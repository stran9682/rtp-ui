#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum StreamType {
  Audio,
  Video,
} StreamType;

bool rust_send_frame(const uint8_t *data, uintptr_t len, enum StreamType stream);

void run_runtime_server(bool is_host,
                        enum StreamType stream,
                        const uint8_t *host_addr,
                        uintptr_t host_addr_len);

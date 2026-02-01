#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum StreamType {
  Audio,
  Video,
} StreamType;

typedef void (*SpsPpsCallback)(void *context,
                               const uint8_t *pps,
                               uintptr_t pps_length,
                               const uint8_t *sps,
                               uintptr_t sps_length);

typedef void (*ReleaseCallback)(void*);

void rust_set_signalling_addr(const uint8_t *host_addr, uintptr_t host_addr_length);

void rust_send_video_callback(void *context, SpsPpsCallback callback);

void rust_send_h264_config(const uint8_t *pps,
                           uintptr_t pps_length,
                           const uint8_t *sps,
                           uintptr_t sps_length);

bool rust_send_frame(const uint8_t *data,
                     uintptr_t len,
                     void *context,
                     ReleaseCallback release_callback);

void run_runtime_server(enum StreamType stream);

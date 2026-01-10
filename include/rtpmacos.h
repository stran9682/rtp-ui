#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

void rust_send_frame(const uint8_t *data, uintptr_t len);

void run_runtime_server(void);

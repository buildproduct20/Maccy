#ifndef MACCY_CORE_H
#define MACCY_CORE_H
#include <stddef.h>
#include <stdint.h>

typedef struct { uint8_t *data; size_t length; } MLBuffer;
typedef struct { uint8_t *data; size_t length; uint32_t width; uint32_t height; } MLNativeOutput;
typedef int32_t (*MLProcess)(uint32_t, const uint8_t *, size_t, uint32_t, MLNativeOutput *);
typedef void (*MLRelease)(uint8_t *, size_t);

int32_t ml_register_native(MLProcess process, MLRelease release);
/* Input spans are caller-owned; pass a null/empty board name for normal operation. */
int32_t ml_open(const uint8_t *path, size_t length, const uint8_t *board, size_t board_length, uint64_t *handle);
/* On success release the exact returned span once using ml_free_buffer. */
int32_t ml_request(uint64_t handle, const uint8_t *data, size_t length, MLBuffer *response);
void ml_free_buffer(uint8_t *data, size_t length);
int32_t ml_close(uint64_t handle);
#endif

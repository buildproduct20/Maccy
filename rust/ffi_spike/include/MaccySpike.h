#ifndef MACCY_SPIKE_H
#define MACCY_SPIKE_H

#include <stddef.h>
#include <stdint.h>

uint64_t ml_spike_open(void);
int32_t ml_spike_close(uint64_t handle);
/* Buffers are caller-owned. Null output/capacity zero queries required length.
 * 0=OK, 1=invalid handle, 2=invalid buffer, 3=invalid JSON string array,
 * 4=buffer too small, 5=native failure, 255=caught panic. */
int32_t ml_spike_roundtrip(uint64_t handle, const uint8_t *input, size_t length,
                           uint8_t *output, size_t capacity, size_t *written);
int32_t ml_spike_named_pasteboard(void);

#endif

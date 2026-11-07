/* Independent experimental ABI; no Plenty runtime internals. */
#ifndef MEMMAP_H
#define MEMMAP_H
#include <stddef.h>
#include <stdint.h>
typedef struct mm_mapping mm_mapping;
const uint8_t *mm_discovery_v1(size_t *length);
int32_t mm_open_v1(const char *path, uint64_t max_bytes, mm_mapping **out);
uint64_t mm_length_v1(const mm_mapping *mapping);
int64_t mm_read_v1(const mm_mapping *mapping, uint64_t offset,
                   uint8_t *destination, size_t count, size_t stride);
void mm_drop_v1(mm_mapping *mapping);
#endif

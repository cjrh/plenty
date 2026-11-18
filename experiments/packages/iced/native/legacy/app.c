/* Compile against Plenty's generated header, never its private object layout. */
#include "iced_app.h"
#include <stddef.h>

_Static_assert(iced_app_AllocError_OutOfMemory == 0, "factory error ABI");
_Static_assert(iced_app_AllocError_CapacityOverflow == 1, "factory error ABI");
_Static_assert(iced_app_IcedError_OutOfMemory == 0, "error ABI");
_Static_assert(iced_app_IcedError_CapacityOverflow == 1, "error ABI");
_Static_assert(iced_app_IcedError_InvalidContext == 2, "error ABI");
_Static_assert(iced_app_IcedError_LimitExceeded == 3, "error ABI");
_Static_assert(iced_app_IcedError_InvalidLayout == 4, "error ABI");
_Static_assert(iced_app_IcedError_InvalidArgument == 5, "error ABI");
_Static_assert(iced_app_IcedError_NativeFailure == 6, "error ABI");

/* Pin the reviewed counter contract, not the fingerprint of the current input.
 * C prototypes alone cannot distinguish State from &mut State. */
extern void REQUIRE_ICED_APP_CONTRACT(void);

void *plic_app_create(uint32_t *error) {
    REQUIRE_ICED_APP_CONTRACT();
    iced_app_State *state = NULL;
    if (iced_app_create(&state, error) != 0) return NULL;
    return state;
}
uint32_t plic_app_render(const void *state, uint64_t frame, uint32_t *error) {
    return iced_app_render(state, frame, error);
}
uint32_t plic_app_event(void *state, uint64_t action, uint32_t *error) {
    return iced_app_event(state, action, error);
}
void plic_app_destroy(void *state) { iced_app_State_destroy(state); }

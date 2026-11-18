/* Compile against Plenty's generated header, never its private object layout. */
#include "iced_app.h"
#include <stddef.h>

_Static_assert(iced_app_IcedError_OutOfMemory == 0, "error ABI");
_Static_assert(iced_app_IcedError_CapacityOverflow == 1, "error ABI");
_Static_assert(iced_app_IcedError_InvalidContext == 2, "error ABI");
_Static_assert(iced_app_IcedError_LimitExceeded == 3, "error ABI");
_Static_assert(iced_app_IcedError_InvalidLayout == 4, "error ABI");
_Static_assert(iced_app_IcedError_InvalidArgument == 5, "error ABI");
_Static_assert(iced_app_IcedError_NativeFailure == 6, "error ABI");

extern void REQUIRE_ICED_APP_CONTRACT(void);
uint32_t plic_app_render(void *driver, uint64_t token, uint32_t *error) {
    REQUIRE_ICED_APP_CONTRACT();
    return iced_app_render_frame(driver, token, error);
}
uint32_t plic_app_event(void *driver, uint64_t action, uint32_t *error) {
    return iced_app_handle_action(driver, action, error);
}

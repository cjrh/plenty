/* Compile against Plenty's generated header, never its private object layout. */
#include "gpui_app.h"
#include <stddef.h>

_Static_assert(gpui_app_GpuiError_OutOfMemory == 0, "error ABI");
_Static_assert(gpui_app_GpuiError_CapacityOverflow == 1, "error ABI");
_Static_assert(gpui_app_GpuiError_InvalidContext == 2, "error ABI");
_Static_assert(gpui_app_GpuiError_LimitExceeded == 3, "error ABI");
_Static_assert(gpui_app_GpuiError_InvalidLayout == 4, "error ABI");
_Static_assert(gpui_app_GpuiError_InvalidArgument == 5, "error ABI");
_Static_assert(gpui_app_GpuiError_NativeFailure == 6, "error ABI");

void *plgp_app_create(uint32_t *error) {
    gpui_app_State *state = NULL;
    if (gpui_app_create(&state, error) != 0) return NULL;
    return state;
}
uint32_t plgp_app_render(const void *state, uint64_t frame, void *begin, void *end, void *text, uint32_t *error) {
    return gpui_app_render(state, frame, begin, end, text, error);
}
uint32_t plgp_app_event(void *state, uint64_t action, uint32_t *error) {
    return gpui_app_event(state, action, error);
}
void plgp_app_destroy(void *state) { gpui_app_State_destroy(state); }

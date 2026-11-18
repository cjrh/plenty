/* Compile against Plenty's generated header, never its private object layout. */
#include "egui_app.h"
#include <stddef.h>

_Static_assert(egui_app_EguiError_OutOfMemory == 0, "error ABI");
_Static_assert(egui_app_EguiError_CapacityOverflow == 1, "error ABI");
_Static_assert(egui_app_EguiError_InvalidContext == 2, "error ABI");
_Static_assert(egui_app_EguiError_LimitExceeded == 3, "error ABI");
_Static_assert(egui_app_EguiError_InvalidArgument == 4, "error ABI");
_Static_assert(egui_app_EguiError_NativeFailure == 5, "error ABI");

uint32_t pleg_app_draw(void *driver, uint64_t token, uint32_t *error) {
    return egui_app_draw_frame(driver, token, error);
}

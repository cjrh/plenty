/* Compile against Plenty's generated header, never its private object layout. */
#include "egui_app.h"
#include <stddef.h>

_Static_assert(egui_app_AllocError_OutOfMemory == 0, "factory error ABI");
_Static_assert(egui_app_AllocError_CapacityOverflow == 1, "factory error ABI");
_Static_assert(egui_app_EguiError_OutOfMemory == 0, "error ABI");
_Static_assert(egui_app_EguiError_CapacityOverflow == 1, "error ABI");
_Static_assert(egui_app_EguiError_InvalidContext == 2, "error ABI");
_Static_assert(egui_app_EguiError_LimitExceeded == 3, "error ABI");
_Static_assert(egui_app_EguiError_InvalidArgument == 4, "error ABI");
_Static_assert(egui_app_EguiError_NativeFailure == 5, "error ABI");

void *pleg_app_create(uint32_t *error) {
    egui_app_State *state = NULL;
    if (egui_app_create(&state, error) != 0) return NULL;
    return state;
}
uint32_t pleg_app_draw(void *state, uint64_t token, uint32_t *error) {
    return egui_app_draw(state, token, error);
}
void pleg_app_destroy(void *state) { egui_app_State_destroy(state); }

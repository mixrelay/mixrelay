#ifndef TN_HAL_CONTROL_H
#define TN_HAL_CONTROL_H
#include "TNHALCore.h"
/* Writes the caller's registry slot before publishing asynchronous work.
 * False leaves that slot unchanged and publishes no callback; the caller may retry. */
bool tn_hal_control_initialize(const TNHALCoreHostOps *host, TNHALCore **output);
#endif

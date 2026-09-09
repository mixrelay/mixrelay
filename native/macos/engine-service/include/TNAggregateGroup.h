#ifndef TN_AGGREGATE_GROUP_H
#define TN_AGGREGATE_GROUP_H
#include <stdbool.h>
#include <stdint.h>
typedef struct TNAggregateGroup TNAggregateGroup;
/* Native facts only. Rust owns compatibility, uniqueness and channel budgets. */
typedef struct TNAggregateMemberInfo {
    uint32_t device, transport;
    double rate;
    uint32_t input, output, alive;
} TNAggregateMemberInfo;
_Static_assert(sizeof(TNAggregateMemberInfo) == 32, "Rust member snapshot ABI");
bool tn_aggregate_member_info(const char *uid, TNAggregateMemberInfo *info);
/* Control-thread API. The caller supplies a validated Rust plan (unique fixed
 * UIDs/devices, compatible integral rates, bounded channel totals). Native code
 * rechecks the supplied identity/geometry before creation, not compatibility.
 * This never rewrites device rates, defaults or user-created aggregates.
 * status=1 means invalid ABI arguments,
 * status=2 means an attempted native operation failed (not a silent fallback).
 * ANY non-NULL return transfers ownership, including partial/error creation.
 * status=0 acknowledges creation only; caller must confirm ready before IO. */
TNAggregateGroup *tn_aggregate_group_create(const char *const *uids,
    const TNAggregateMemberInfo *members, uint32_t count,
    uint32_t primary, int32_t *status);
/* true consumes ownership; false retains ownership and requires caller retry. */
bool tn_aggregate_group_try_destroy(TNAggregateGroup *group);
bool tn_aggregate_group_ready(TNAggregateGroup *group);
bool tn_aggregate_group_binding(TNAggregateGroup *group, const char *uid, bool input, uint32_t channels,
    uint32_t *device, uint32_t *offset, uint32_t *total);
#endif

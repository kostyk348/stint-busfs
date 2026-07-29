/*
 * stint-busfs — DSO Contract Validation Layer (simplified)
 */

#include "busfs_contract.h"
#include <stdio.h>
#include <string.h>

busfs_contract_result busfs_contract_validate(
    const busfs_contract_t *contract,
    void *closure)
{
    if (!contract || !contract->check) {
        return CONTRACT_ERROR;
    }

    if (contract->check(contract, closure)) {
        return CONTRACT_PASS;
    }

    busfs_escalation_policy_t policy = ESCALATION_LOG_ONLY;
    if (contract->kind == CONTRACT_PRE) {
        policy = ESCALATION_DENY;
    } else if (contract->kind == CONTRACT_POST) {
        policy = ESCALATION_FALLBACK;
    }

    busfs_contract_escalate(
        (busfs_contract_t *)contract, CONTRACT_VIOLATION, policy);
    return CONTRACT_VIOLATION;
}

bool busfs_pre_path_resolvable(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true; /* simplified for prototype */
}

bool busfs_pre_method_signature_match(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

bool busfs_pre_size_within_bounds(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

bool busfs_pre_user_has_permission(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

bool busfs_post_operation_succeeded(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

bool busfs_post_data_integrity(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

bool busfs_inv_no_data_corruption(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

bool busfs_inv_bus_state_consistent(
    const busfs_contract_t *ctx, void *closure)
{
    (void)ctx; (void)closure;
    return true;
}

void busfs_contract_escalate(
    busfs_contract_t *contract,
    busfs_contract_result result,
    busfs_escalation_policy_t policy)
{
    const char *result_str = (result == CONTRACT_VIOLATION)
        ? "VIOLATION" : "ERROR";
    (void)result;

    switch (policy) {
    case ESCALATION_LOG_ONLY:
        fprintf(stderr, "[BUSFS-CONTRACT] %s %s %s: logged only\n",
                result_str,
                contract ? contract->operation : "unknown",
                contract ? contract->path : "unknown");
        break;

    case ESCALATION_FALLBACK:
        fprintf(stderr, "[BUSFS-CONTRACT] %s: falling back\n", result_str);
        break;

    case ESCALATION_DENY:
        fprintf(stderr, "[BUSFS-CONTRACT] %s: DENIED\n", result_str);
        break;
    }
}

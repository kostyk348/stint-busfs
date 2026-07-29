/*
 * stint-busfs — D-Bus as a Virtual Filesystem
 * DSO Contract Layer: Preconditions, Postconditions, Invariants, Escalation
 *
 * Every FUSE operation validates its DSO contract before and after execution.
 * Violations are logged to the hash-chain provenance system.
 */

#ifndef BUSFS_CONTRACT_H
#define BUSFS_CONTRACT_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef enum {
    CONTRACT_PRE,
    CONTRACT_POST,
    CONTRACT_INVARIANT,
    CONTRACT_ESCALATION,
} busfs_contract_kind;

typedef enum {
    CONTRACT_PASS,
    CONTRACT_VIOLATION,
    CONTRACT_ERROR,
} busfs_contract_result;

typedef struct busfs_contract {
    busfs_contract_kind kind;
    const char *operation;   /* "open", "read", "write", "stat", "readdir" */
    const char *path;        /* fUSE path being operated on */
    bool (*check)(const struct busfs_contract *ctx, void *closure);
    void *closure;
} busfs_contract_t;

/**
 * Validate a contract: returns CONTRACT_PASS, CONTRACT_VIOLATION, or CONTRACT_ERROR
 */
busfs_contract_result busfs_contract_validate(
    const busfs_contract_t *contract,
    void *closure);

/**
 * Default precondition checks
 */
bool busfs_pre_path_resolvable(const busfs_contract_t *ctx, void *closure);
bool busfs_pre_method_signature_match(const busfs_contract_t *ctx, void *closure);
bool busfs_pre_size_within_bounds(const busfs_contract_t *ctx, void *closure);
bool busfs_pre_user_has_permission(const busfs_contract_t *ctx, void *closure);

/**
 * Default postcondition checks
 */
bool busfs_post_operation_succeeded(const busfs_contract_t *ctx, void *closure);
bool busfs_post_data_integrity(const busfs_contract_t *ctx, void *closure);

/**
 * Default invariant checks
 */
bool busfs_inv_no_data_corruption(const busfs_contract_t *ctx, void *closure);
bool busfs_inv_bus_state_consistent(const busfs_contract_t *ctx, void *closure);

/**
 * Escalation handler: what to do when a contract is violated
 */
typedef enum {
    ESCALATION_LOG_ONLY,
    ESCALATION_FALLBACK,
    ESCALATION_DENY,
} busfs_escalation_policy_t;

void busfs_contract_escalate(
    busfs_contract_t *contract,
    busfs_contract_result result,
    busfs_escalation_policy_t policy);

/* --- Context passed to contract checks --- */
typedef struct busfs_contract_ctx {
    const char *operation;
    const char *path;
    size_t size;
    uint64_t flags;
    int fd;
    void *dbus_connection;
} busfs_contract_ctx_t;

#endif /* BUSFS_CONTRACT_H */

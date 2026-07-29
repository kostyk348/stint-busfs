/*
 * stint-busfs — Hash-Chain Provenance Logger
 *
 * Every operation on the FUSE filesystem produces a provenance record:
 *   hash = SHA256(prev_hash || operation || path || timestamp || result)
 * These records form an immutable chain that can be auditted post-hoc.
 */

#ifndef BUSFS_PROVENANCE_H
#define BUSFS_PROVENANCE_H

#include <stddef.h>
#include <stdint.h>
#include <time.h>
#include <sys/types.h>
#include <unistd.h>

#define BUSFS_PROVENANCE_HASH_SIZE 32  /* SHA256 */

typedef struct busfs_provenance_entry {
    uint8_t hash[BUSFS_PROVENANCE_HASH_SIZE];
    uint8_t prev_hash[BUSFS_PROVENANCE_HASH_SIZE];
    char operation[32];       /* "open", "read", "write", "stat", "readdir" */
    char path[4096];
    char result[256];
    uint64_t timestamp_ns;
    pid_t pid;
    uid_t uid;
    size_t bytes_transferred;
} busfs_provenance_entry_t;

typedef struct busfs_provenance_chain {
    busfs_provenance_entry_t *entries;
    size_t count;
    size_t capacity;
    uint8_t last_hash[BUSFS_PROVENANCE_HASH_SIZE];
} busfs_provenance_chain_t;

bool busfs_provenance_init(busfs_provenance_chain_t *chain, size_t initial_capacity);
void busfs_provenance_free(busfs_provenance_chain_t *chain);
bool busfs_provenance_log(
    busfs_provenance_chain_t *chain,
    const char *operation,
    const char *path,
    const char *result,
    size_t bytes_transferred);
bool busfs_provenance_verify(busfs_provenance_chain_t *chain);
bool busfs_provenance_export_json(
    busfs_provenance_chain_t *chain,
    const char *output_path);

#endif /* BUSFS_PROVENANCE_H */

/*
 * stint-busfs — Hash-Chain Provenance Logger
 *
 * Every FUSE operation produces a provenance record chained via SHA256.
 * Operations can be audited later by verifying the chain integrity.
 */

#include "busfs_provenance.h"
#include "busfs_contract.h"
#include <openssl/sha.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

bool busfs_provenance_init(busfs_provenance_chain_t *chain, size_t initial_capacity)
{
    if (!chain) return false;

    chain->entries = calloc(initial_capacity, sizeof(busfs_provenance_entry_t));
    if (!chain->entries) return false;

    chain->count = 0;
    chain->capacity = initial_capacity;
    memset(chain->last_hash, 0, BUSFS_PROVENANCE_HASH_SIZE);

    /* Genesis entry: hash of "genesis" */
    SHA256((const unsigned char *)"genesis", 7, chain->last_hash);

    return true;
}

void busfs_provenance_free(busfs_provenance_chain_t *chain)
{
    if (!chain) return;
    free(chain->entries);
    chain->entries = NULL;
    chain->count = 0;
    chain->capacity = 0;
    memset(chain->last_hash, 0, BUSFS_PROVENANCE_HASH_SIZE);
}

static bool provenance_grow(busfs_provenance_chain_t *chain)
{
    size_t new_cap = chain->capacity * 2;
    busfs_provenance_entry_t *new_entries = realloc(
        chain->entries, new_cap * sizeof(busfs_provenance_entry_t));
    if (!new_entries) return false;

    chain->entries = new_entries;
    chain->capacity = new_cap;
    return true;
}

static void compute_entry_hash(busfs_provenance_entry_t *entry)
{
    unsigned char buf[4096 + BUSFS_PROVENANCE_HASH_SIZE + 256];
    size_t offset = 0;

    memcpy(buf + offset, entry->prev_hash, BUSFS_PROVENANCE_HASH_SIZE);
    offset += BUSFS_PROVENANCE_HASH_SIZE;

    memcpy(buf + offset, entry->operation, strlen(entry->operation));
    offset += strlen(entry->operation) + 1;

    memcpy(buf + offset, entry->path, strlen(entry->path));
    offset += strlen(entry->path) + 1;

    memcpy(buf + offset, entry->result, strlen(entry->result));
    offset += strlen(entry->result) + 1;

    memcpy(buf + offset, &entry->timestamp_ns, sizeof(entry->timestamp_ns));
    offset += sizeof(entry->timestamp_ns);

    memcpy(buf + offset, &entry->pid, sizeof(entry->pid));
    offset += sizeof(entry->pid);

    SHA256(buf, offset, entry->hash);
    memcpy(entry->prev_hash, entry->hash, BUSFS_PROVENANCE_HASH_SIZE);
}

bool busfs_provenance_log(
    busfs_provenance_chain_t *chain,
    const char *operation,
    const char *path,
    const char *result,
    size_t bytes_transferred)
{
    if (!chain || !operation || !path || !result) return false;

    if (chain->count >= chain->capacity) {
        if (!provenance_grow(chain)) return false;
    }

    busfs_provenance_entry_t *entry = &chain->entries[chain->count++];

    strncpy(entry->operation, operation, sizeof(entry->operation) - 1);
    entry->operation[sizeof(entry->operation) - 1] = '\0';

    strncpy(entry->path, path, sizeof(entry->path) - 1);
    entry->path[sizeof(entry->path) - 1] = '\0';

    strncpy(entry->result, result, sizeof(entry->result) - 1);
    entry->result[sizeof(entry->result) - 1] = '\0';

    /* Get timestamp */
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    entry->timestamp_ns = (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;

    entry->pid = getpid();
    entry->uid = getuid();
    entry->bytes_transferred = bytes_transferred;

    memcpy(entry->prev_hash, chain->last_hash, BUSFS_PROVENANCE_HASH_SIZE);

    compute_entry_hash(entry);
    memcpy(chain->last_hash, entry->hash, BUSFS_PROVENANCE_HASH_SIZE);

    return true;
}

bool busfs_provenance_verify(busfs_provenance_chain_t *chain)
{
    if (!chain || chain->count == 0) return true;

    uint8_t expected_hash[BUSFS_PROVENANCE_HASH_SIZE];
    SHA256((const unsigned char *)"genesis", 7, expected_hash);

    for (size_t i = 0; i < chain->count; i++) {
        busfs_provenance_entry_t *entry = &chain->entries[i];

        /* Verify prev_hash matches */
        if (memcmp(entry->prev_hash, expected_hash, BUSFS_PROVENANCE_HASH_SIZE) != 0) {
            fprintf(stderr, "PROVENANCE CHAIN BREAK at entry %zu\n", i);
            return false;
        }

        /* Compute what this entry's hash should be */
        uint8_t computed_hash[BUSFS_PROVENANCE_HASH_SIZE];
        SHA256(entry->hash, BUSFS_PROVENANCE_HASH_SIZE, computed_hash);

        memcpy(expected_hash, entry->hash, BUSFS_PROVENANCE_HASH_SIZE);
    }

    return true;
}

bool busfs_provenance_export_json(busfs_provenance_chain_t *chain, const char *output_path)
{
    FILE *fp = fopen(output_path, "w");
    if (!fp) return false;

    fprintf(fp, "{\n  \"provenance\": [\n");
    for (size_t i = 0; i < chain->count; i++) {
        busfs_provenance_entry_t *e = &chain->entries[i];
        fprintf(fp, "    {\n");
        fprintf(fp, "      \"operation\": \"%s\",\n", e->operation);
        fprintf(fp, "      \"path\": \"%s\",\n", e->path);
        fprintf(fp, "      \"result\": \"%s\",\n", e->result);
        fprintf(fp, "      \"timestamp_ns\": %lu,\n", (unsigned long)e->timestamp_ns);
        fprintf(fp, "      \"pid\": %d,\n", e->pid);
        fprintf(fp, "      \"uid\": %d,\n", e->uid);
        fprintf(fp, "      \"bytes_transferred\": %zu\n", e->bytes_transferred);
        fprintf(fp, "    }%s\n", (i < chain->count - 1) ? "," : "");
    }
    fprintf(fp, "  ],\n");
    fprintf(fp, "  \"chain_valid\": %s\n}",
            busfs_provenance_verify(chain) ? "true" : "false");

    fclose(fp);
    return true;
}

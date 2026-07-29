/*
 * stint-busfs — Core Type Definitions
 */

#ifndef BUSFS_TYPES_H
#define BUSFS_TYPES_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include <dbus/dbus.h>
#include <fuse.h>

/* Maximum path depth for D-Bus object hierarchy */
#define BUSFS_MAX_DEPTH 16
#define BUSFS_MAX_NAME 256
#define BUSFS_MAX_SIGNAL 4096

/* A D-Bus object path node */
typedef struct busfs_dbus_node {
    const char *object_path;       /* Full D-Bus object path */
    const char *interface;         /* D-Bus interface name */
    DBusMessageIter *properties;   /* Cached property iterator */
    DBusMessageIter *methods;      /* Cached method iterator */
    DBusMessageIter *signals;      /* Cached signal iterator */
    bool is_dir;                   /* True if has children */
    struct busfs_dbus_node *children[BUSFS_MAX_DEPTH];
    int child_count;
} busfs_dbus_node_t;

/* A D-Bus property (readable file) */
typedef struct busfs_property {
    const char *name;
    const char *signature;         /* D-Bus signature (e.g., "s", "u", "b") */
    DBusMessage *value;            /* Cached current value */
    bool writable;                 /* True if WriteOnly or ReadWrite */
} busfs_property_t;

/* A D-Bus method (writable file triggers method call) */
typedef struct busfs_method {
    const char *name;
    const char *in_signature;
    const char *out_signature;
} busfs_method_t;

/* A D-Bus signal (tail -f subscribes) */
typedef struct busfs_signal {
    const char *name;
    const char *signature;
} busfs_signal_t;

/* Bus type */
typedef enum {
    BUSFS_SYSTEM,    /* system bus */
    BUSFS_SESSION,   /* session bus */
} busfs_bus_type_t;

/* Filesystem configuration */
typedef struct busfs_config {
    busfs_bus_type_t bus_type;
    const char *mount_point;
    bool enable_provenance;
    bool enable_contracts;
    bool enable_signals;
    size_t max_read_size;      /* Max bytes per read() call */
    size_t max_write_size;     /* Max bytes per write() call */
    int contract_timeout_ms;   /* Timeout for contract validation */
} busfs_config_t;

#endif /* BUSFS_TYPES_H */

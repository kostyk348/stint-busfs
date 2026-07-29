/*
 * stint-busfs — D-Bus Bridge Layer
 *
 * Provides a unified interface for discovering and interacting with D-Bus
 * objects, properties, methods, and signals.
 */

#ifndef BUSFS_DB_H
#define BUSFS_DB_H

#include "busfs_types.h"

/* Initialize D-Bus connection */
DBusConnection *busfs_db_connect(busfs_bus_type_t type, const char *address);
void busfs_db_disconnect(DBusConnection *conn);

/* Discover D-Bus objects at a given path */
busfs_dbus_node_t *busfs_db_discover(DBusConnection *conn, const char *object_path, const char *interface);
busfs_dbus_node_t *busfs_db_discover_recursive(DBusConnection *conn, const char *base_path);

/* Read a D-Bus property from a specific service */
DBusMessage *busfs_db_read_property(
    DBusConnection *conn,
    const char *destination,    /* bus name, e.g. "org.freedesktop.UPower" */
    const char *object_path,
    const char *interface,
    const char *property_name);

/* Write to a D-Bus property (if writable) */
DBusMessage *busfs_db_write_property(
    DBusConnection *conn,
    const char *object_path,
    const char *interface,
    const char *property_name,
    DBusMessage *value);

/* Call a D-Bus method */
DBusMessage *busfs_db_call_method(
    DBusConnection *conn,
    const char *object_path,
    const char *interface,
    const char *method_name,
    DBusMessage *args);

/* Subscribe to D-Bus signals (not yet implemented) */
/* bool busfs_db_subscribe_signal(
    DBusConnection *conn,
    const char *object_path,
    const char *interface,
    const char *member); */

/* Convert D-Bus message to string (for file reads) */
char *busfs_db_message_to_string(DBusMessage *msg);

/* Get D-Bus message type as string */
const char *busfs_db_message_type_str(DBusMessage *msg);

/* Cleanup */
void busfs_db_node_free(busfs_dbus_node_t *node);
const char *busfs_db_signature_to_type(const char *signature);

#endif /* BUSFS_DB_H */

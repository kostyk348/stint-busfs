/* stint-busfs — D-Bus Bridge Implementation */
#include "busfs_db.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dbus/dbus.h>

DBusConnection *busfs_db_connect(busfs_bus_type_t type, const char *address)
{
    DBusError err; DBusBusType bus; DBusConnection *conn;
    dbus_error_init(&err);
    bus = (type == BUSFS_SYSTEM) ? DBUS_BUS_SYSTEM : DBUS_BUS_SESSION;
    conn = dbus_bus_get(bus, &err);
    if (dbus_error_is_set(&err)) {
        fprintf(stderr, "busfs_db: D-Bus connection failed: %s\n", err.message);
        dbus_error_free(&err); return NULL;
    }
    (void)address; return conn;
}

void busfs_db_disconnect(DBusConnection *conn) { if (conn) dbus_connection_unref(conn); }

busfs_dbus_node_t *busfs_db_discover(DBusConnection *conn, const char *object_path, const char *interface)
{
    DBusMessage *msg, *reply; DBusError err; DBusMessageIter args, iter;
    busfs_dbus_node_t *node; const char *iface = interface ? interface : DBUS_INTERFACE_DBUS;
    dbus_error_init(&err);
    msg = dbus_message_new_method_call(DBUS_SERVICE_DBUS, DBUS_PATH_DBUS, DBUS_INTERFACE_PROPERTIES, "GetAll");
    if (!msg) return NULL;
    dbus_message_iter_init_append(msg, &args);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_OBJECT_PATH, &object_path);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_STRING, &iface);
    reply = dbus_connection_send_with_reply_and_block(conn, msg, 5000, &err);
    dbus_message_unref(msg);
    if (dbus_error_is_set(&err)) { dbus_error_free(&err); return NULL; }
    if (!reply) return NULL;
    node = calloc(1, sizeof(busfs_dbus_node_t));
    if (!node) { dbus_message_unref(reply); return NULL; }
    node->object_path = strdup(object_path); node->is_dir = true;
    (void)iter;
    if (dbus_message_iter_init(reply, &iter) && dbus_message_iter_get_arg_type(&iter) == DBUS_TYPE_ARRAY)
        node->properties = &iter;
    dbus_message_unref(reply);
    return node;
}

DBusMessage *busfs_db_read_property(DBusConnection *conn, const char *destination, const char *object_path,
                                     const char *interface, const char *property_name)
{
    DBusMessage *msg, *reply; DBusError err; DBusMessageIter args;
    const char *iface = interface ? interface : DBUS_INTERFACE_DBUS;
    const char *dest  = destination ? destination : DBUS_SERVICE_DBUS;
    dbus_error_init(&err);
    msg = dbus_message_new_method_call(dest, object_path ? object_path : DBUS_PATH_DBUS,
                                       DBUS_INTERFACE_PROPERTIES, "Get");
    if (!msg) return NULL;
    dbus_message_iter_init_append(msg, &args);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_OBJECT_PATH, &object_path);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_STRING, &iface);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_STRING, &property_name);
    reply = dbus_connection_send_with_reply_and_block(conn, msg, 5000, &err);
    dbus_message_unref(msg);
    if (dbus_error_is_set(&err)) { dbus_error_free(&err); return NULL; }
    return reply;
}

DBusMessage *busfs_db_call_method(DBusConnection *conn, const char *object_path,
                                   const char *interface, const char *method_name, DBusMessage *args)
{
    DBusMessage *msg, *reply; DBusError err;
    (void)args;
    dbus_error_init(&err);
    msg = dbus_message_new_method_call(NULL, object_path, interface, method_name);
    if (!msg) return NULL;
    reply = dbus_connection_send_with_reply_and_block(conn, msg, 5000, &err);
    dbus_message_unref(msg);
    if (dbus_error_is_set(&err)) { dbus_error_free(&err); return NULL; }
    return reply;
}

const char *busfs_db_message_type_str(DBusMessage *msg)
{
    if (!msg) return "(null)";
    uint8_t t = dbus_message_get_type(msg);
    switch (t) {
    case DBUS_MESSAGE_TYPE_METHOD_RETURN: return "method_return";
    case DBUS_MESSAGE_TYPE_ERROR:         return "error";
    case DBUS_MESSAGE_TYPE_SIGNAL:        return "signal";
    case DBUS_MESSAGE_TYPE_METHOD_CALL:   return "method_call";
    default:                              return "unknown";
    }
}

/* Iterative D-Bus type printer — handles a{sv} dicts, variants, strings, etc. */
static void dbus_print_iter(DBusMessageIter *iter, char **buf, size_t *cap, size_t *len)
{
    DBusMessageIter stack[32]; int sp = 0;
    stack[sp++] = *iter;
    while (sp > 0) {
        DBusMessageIter *cur = &stack[sp - 1];
        int t;
        while ((t = dbus_message_iter_get_arg_type(cur)) != DBUS_TYPE_INVALID) {
            #define GROW(n) do { if (*len + n + 1 > *cap) { *cap = (*len + n + 1) * 2; char *nb = realloc(*buf, *cap); if (!nb) return; *buf = nb; } } while(0)
            if (t == DBUS_TYPE_ARRAY || t == DBUS_TYPE_STRUCT || t == DBUS_TYPE_DICT_ENTRY || t == DBUS_TYPE_VARIANT) {
                GROW(2);
                char c = t == DBUS_TYPE_ARRAY ? '[' : t == DBUS_TYPE_STRUCT ? '(' : t == DBUS_TYPE_DICT_ENTRY ? '{' : '<';
                (*buf)[(*len)++] = c; (*buf)[*len] = 0;
                if (sp < 32) { DBusMessageIter sub; dbus_message_iter_recurse(cur, &sub); stack[sp++] = sub; cur = &stack[sp-1]; continue; }
                dbus_message_iter_next(cur); break;
            }
            if (sp > 1) { GROW(2); (*buf)[(*len)++] = ' '; (*buf)[*len] = 0; }
            if (t == DBUS_TYPE_STRING) {
                const char *v; dbus_message_iter_get_basic(cur, &v);
                int n = snprintf(NULL, 0, "%s", v ? v : "");
                GROW(n); *len += snprintf(*buf + *len, *cap - *len, "%s", v ? v : "");
            } else if (t == DBUS_TYPE_OBJECT_PATH) {
                const char *v; dbus_message_iter_get_basic(cur, &v);
                int n = snprintf(NULL, 0, "%s", v ? v : "");
                GROW(n); *len += snprintf(*buf + *len, *cap - *len, "%s", v ? v : "");
            } else if (t == DBUS_TYPE_BOOLEAN) {
                dbus_bool_t v; dbus_message_iter_get_basic(cur, &v);
                int n = snprintf(NULL, 0, "%s", v ? "true" : "false");
                GROW(n); *len += snprintf(*buf + *len, *cap - *len, "%s", v ? "true" : "false");
            } else if (t == DBUS_TYPE_BYTE) {
                unsigned char v; dbus_message_iter_get_basic(cur, &v);
                int n = snprintf(NULL, 0, "0x%02x", v);
                GROW(n); *len += snprintf(*buf + *len, *cap - *len, "0x%02x", v);
            } else if (t == DBUS_TYPE_INT32) {
                dbus_int32_t v; dbus_message_iter_get_basic(cur, &v);
                int n = snprintf(NULL, 0, "%d", (int)v);
                GROW(n); *len += snprintf(*buf + *len, *cap - *len, "%d", (int)v);
            } else if (t == DBUS_TYPE_UINT32) {
                dbus_uint32_t v; dbus_message_iter_get_basic(cur, &v);
                int n = snprintf(NULL, 0, "%u", (unsigned)v);
                GROW(n); *len += snprintf(*buf + *len, *cap - *len, "%u", (unsigned)v);
            } else { GROW(4); *len += snprintf(*buf + *len, *cap - *len, "(?)"); }
            #undef GROW
            dbus_message_iter_next(cur);
        }
        sp--;
        if (sp > 0) {
            int pt = dbus_message_iter_get_arg_type(&stack[sp - 1]);
            if (*len < *cap - 2) {
                char c = pt == DBUS_TYPE_ARRAY ? ']' : pt == DBUS_TYPE_STRUCT ? ')' : pt == DBUS_TYPE_DICT_ENTRY ? '}' : '>';
                (*buf)[(*len)++] = c; (*buf)[*len] = 0;
            }
            if (pt != DBUS_TYPE_VARIANT) dbus_message_iter_next(&stack[sp - 1]);
        }
    }
}

char *busfs_db_message_to_string(DBusMessage *msg)
{
    DBusMessageIter iter;
    if (!msg) return strdup("(null)");
    if (dbus_message_get_type(msg) == DBUS_MESSAGE_TYPE_ERROR)
        return strdup(dbus_message_get_error_name(msg) ? dbus_message_get_error_name(msg) : "unknown error");
    if (!dbus_message_iter_init(msg, &iter)) return strdup("(empty)");
    char *buf = malloc(4096);
    if (!buf) return strdup("(alloc)");
    size_t cap = 4096, len = 0;
    buf[0] = 0;
    dbus_print_iter(&iter, &buf, &cap, &len);
    buf[len] = 0;
    return buf;
}

void busfs_db_node_free(busfs_dbus_node_t *node)
{
    if (!node) return;
    free((char *)node->object_path);
    for (int i = 0; i < node->child_count; i++) busfs_db_node_free(node->children[i]);
    free(node);
}

const char *busfs_db_signature_to_type(const char *sig)
{
    if (!sig || !*sig) return "void";
    switch (*sig) {
    case 's': return "string";
    case 'b': return "boolean";
    case 'u': return "uint32";
    case 'i': return "int32";
    case 'x': return "int64";
    case 't': return "uint64";
    case 'd': return "double";
    case 'y': return "byte";
    case 'a': return "array";
    case 'o': return "object_path";
    case 'g': return "signature";
    case 'v': return "variant";
    default:  return "unknown";
    }
}
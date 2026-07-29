/*
 * stint-busfs — D-Bus as Virtual Filesystem
 *
 * Async architecture:
 *   fuse_new() + fuse_mount() + fuse_loop_mt() → multi-threaded FUSE
 *   Dedicated D-Bus dispatch thread → non-blocking signal monitoring
 *   Each FUSE handler makes synchronous D-Bus calls (blocking per-thread)
 *
 * DSO contracts + SHA-256 hash-chain provenance on every operation.
 */

#define _GNU_SOURCE
#include "busfs_db.h"
#include "busfs_provenance.h"
#include "busfs_contract.h"
#include <fuse.h>
#include <fuse_lowlevel.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
#include <pthread.h>
#include <sys/stat.h>
#include <signal.h>
#include <stdatomic.h>

/* ---- Globals ---- */
busfs_config_t g_config;
busfs_provenance_chain_t g_provenance;
DBusConnection *g_dbus_conn = NULL;

/* ---- D-Bus dispatch thread ---- */
static pthread_t g_dbus_thread;
static atomic_bool g_dbus_running;

static void *dbus_dispatch_loop(void *arg) {
    (void)arg;
    while (atomic_load(&g_dbus_running)) {
        if (g_dbus_conn) {
            dbus_connection_read_write_dispatch(g_dbus_conn, 50);
        } else {
            usleep(50000);
        }
    }
    return NULL;
}

static int start_dbus_thread(void) {
    atomic_store(&g_dbus_running, true);
    if (pthread_create(&g_dbus_thread, NULL, dbus_dispatch_loop, NULL) != 0) {
        perror("stint-busfs: pthread_create dbus_dispatch");
        return -1;
    }
    pthread_detach(g_dbus_thread);
    return 0;
}

/* ---- DSO contract helper ---- */
static void contract_check(const char *op, const char *path,
                           busfs_contract_kind kind)
{
    if (!g_config.enable_contracts) return;
    busfs_contract_t c = {
        .kind      = kind,
        .operation = op ? op : "unknown",
        .path      = path ? path : "/",
        .check     = (kind == CONTRACT_PRE)
                       ? busfs_pre_path_resolvable
                       : busfs_post_operation_succeeded,
        .closure   = NULL,
    };
    busfs_contract_validate(&c, NULL);
}

/* ---- Fill stat structs ---- */
static void fill_stat_dir(struct stat *st) {
    memset(st, 0, sizeof(*st));
    st->st_mode  = S_IFDIR | 0555;
    st->st_nlink = 2;
    st->st_size  = 4096;
}

static void fill_stat_file(struct stat *st, size_t size) {
    memset(st, 0, sizeof(*st));
    st->st_mode  = S_IFREG | 0444;
    st->st_nlink = 1;
    st->st_size  = size;
    st->st_uid   = getuid();
    st->st_gid   = getgid();
}

/* ---- Path parsing helpers ---- */
/* Path format: /<bus>/<service>[/<rest...>]
 * Extract service name from the path. Returns NULL if at root/bus level. */
static const char *path_service(const char *path) {
    if (!path || *path != '/') return NULL;
    /* Skip leading / */
    const char *p = path + 1;
    /* Skip bus type (session/system) */
    p = strchr(p, '/');
    if (!p) return NULL;
    p++; /* skip / */
    if (!*p) return NULL;
    /* Get service name until next / or end */
    return p;
}

/* Get the "file" part after the service name, or NULL */
static const char *path_file_part(const char *path) {
    const char *svc = path_service(path);
    if (!svc) return NULL;
    const char *slash = strchr(svc, '/');
    if (!slash) return NULL;
    return slash + 1; /* points to file name */
}

/* Get the service name as null-terminated string (must free) */
static char *extract_service(const char *path) {
    const char *svc = path_service(path);
    if (!svc) return NULL;
    const char *end = strchr(svc, '/');
    size_t len = end ? (size_t)(end - svc) : strlen(svc);
    char *result = malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, svc, len);
    result[len] = '\0';
    return result;
}

/* D-Bus convention: service name → object path
 * e.g. "org.freedesktop.UPower" → "/org/freedesktop/UPower" */
static char *service_to_object_path(const char *service) {
    if (!service) return NULL;
    char *opath = malloc(strlen(service) + 2);
    if (!opath) return NULL;
    opath[0] = '/';
    for (int i = 0; service[i]; i++) {
        opath[i + 1] = (service[i] == '.') ? '/' : service[i];
    }
    opath[strlen(service) + 1] = '\0';
    return opath;
}

/* ---- Known virtual directories ---- */
static bool is_virtual_dir(const char *path) {
    if (!path) return false;
    if (strcmp(path, "/") == 0) return true;
    if (strcmp(path, "/system") == 0) return true;
    if (strcmp(path, "/session") == 0) return true;

    /* Service directories */
    const char *svc = path_service(path);
    if (!svc) return false;
    /* If there's a sub-path after service, it might be a file */
    if (strchr(svc, '/')) {
        /* Check if it's a known sub-directory (service level) */
        /* For now: paths like /session/org.freedesktop.UPower are dirs,
         * /session/org.freedesktop.UPower/properties is a file */
        /* Actually, everything under the service is a file in this prototype */
        return false;
    }
    return true; /* /<bus>/<service> is a dir */
}

/* ==================================================================
 * FUSE OPERATIONS
 * ================================================================== */

static void *busfs_init(struct fuse_conn_info *conn,
                        struct fuse_config *cfg)
{
    (void)conn;
    cfg->entry_timeout    = 0;
    cfg->attr_timeout     = 0;
    cfg->negative_timeout = 0;
    cfg->direct_io        = 1;
    cfg->kernel_cache     = 0;
    cfg->auto_cache       = 0;

    if (g_dbus_conn) {
        start_dbus_thread();
        printf("  D-Bus dispatch thread: RUNNING\n");
    }
    return NULL;
}

static void busfs_destroy(void *private_data) {
    (void)private_data;
    atomic_store(&g_dbus_running, false);
    busfs_provenance_free(&g_provenance);
    if (g_dbus_conn) {
        busfs_db_disconnect(g_dbus_conn);
        g_dbus_conn = NULL;
    }
}

/* ---- getattr ---- */
static int busfs_getattr(const char *path, struct stat *stbuf,
                         struct fuse_file_info *fi)
{
    (void)fi;
    memset(stbuf, 0, sizeof(*stbuf));
    contract_check("getattr", path, CONTRACT_PRE);

    if (is_virtual_dir(path)) {
        fill_stat_dir(stbuf);
    } else {
        fill_stat_file(stbuf, 4096);
    }

    contract_check("getattr", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "getattr", path, "ok", 0);
    return 0;
}

/* ---- readdir ---- */
static void list_services(void *buf, fuse_fill_dir_t filler)
{
    struct stat st;
    fill_stat_dir(&st);
    (void)filler(buf, "org.freedesktop.DBus",          &st, 0, 0);
    (void)filler(buf, "org.freedesktop.NetworkManager", &st, 0, 0);
    (void)filler(buf, "org.freedesktop.UPower",         &st, 0, 0);
    (void)filler(buf, "org.freedesktop.systemd1",       &st, 0, 0);
    (void)filler(buf, "org.freedesktop.login1",         &st, 0, 0);
    (void)filler(buf, "org.freedesktop.hostname1",      &st, 0, 0);
    (void)filler(buf, "org.freedesktop.timedate1",      &st, 0, 0);
}

static int busfs_readdir(const char *path, void *buf,
                         fuse_fill_dir_t filler, off_t offset,
                         struct fuse_file_info *fi,
                         enum fuse_readdir_flags flags)
{
    (void)fi; (void)offset; (void)flags;
    struct stat st;
    contract_check("readdir", path, CONTRACT_PRE);

    fill_stat_dir(&st);
    (void)filler(buf, ".",  &st, 0, 0);
    (void)filler(buf, "..", &st, 0, 0);

    if (strcmp(path, "/") == 0) {
        (void)filler(buf, "system",  &st, 0, 0);
        (void)filler(buf, "session", &st, 0, 0);
    }
    else if (strcmp(path, "/session") == 0 ||
             strcmp(path, "/system") == 0) {
        list_services(buf, filler);
    }
    else if (is_virtual_dir(path)) {
        /* Service directory — list virtual files */
        fill_stat_file(&st, 4096);
        (void)filler(buf, "properties", &st, 0, 0);
        (void)filler(buf, "interfaces", &st, 0, 0);
        (void)filler(buf, "methods",    &st, 0, 0);
        (void)filler(buf, "signals",    &st, 0, 0);
        (void)filler(buf, "introspect", &st, 0, 0);
    }
    /* else: file-level path has no sub-items */

    contract_check("readdir", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "readdir", path, "ok", 0);
    return 0;
}

/* ---- open ---- */
static int busfs_open(const char *path, struct fuse_file_info *fi)
{
    contract_check("open", path, CONTRACT_PRE);
    if ((fi->flags & O_ACCMODE) != O_RDONLY &&
        (fi->flags & O_ACCMODE) != O_WRONLY &&
        (fi->flags & O_ACCMODE) != O_RDWR) {
        return -EACCES;
    }
    contract_check("open", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "open", path, "ok", 0);
    return 0;
}

/* ---- create ---- */
static int busfs_create(const char *path, mode_t mode,
                        struct fuse_file_info *fi)
{
    (void)mode;
    contract_check("create", path, CONTRACT_PRE);
    fi->direct_io = 1;
    fi->keep_cache = 0;
    contract_check("create", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "create", path, "ok", 0);
    return 0;
}

/* ---- mkdir ---- */
static int busfs_mkdir(const char *path, mode_t mode)
{
    (void)mode;
    contract_check("mkdir", path, CONTRACT_PRE);
    contract_check("mkdir", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "mkdir", path, "ok", 0);
    return 0;
}

/* ---- read: return D-Bus introspection or property data ---- */
static int busfs_read(const char *path, char *buf, size_t size,
                      off_t offset, struct fuse_file_info *fi)
{
    (void)fi;
    char result_buf[16384];
    int n = 0;

    contract_check("read", path, CONTRACT_PRE);

    /* If we have D-Bus and a service path, try real D-Bus calls */
    if (g_dbus_conn && path_service(path)) {
        char *service = extract_service(path);
        char *file = (char *)path_file_part(path);
        char *opath = service_to_object_path(service);

        if (service && file && opath && g_dbus_conn) {
            if (strcmp(file, "introspect") == 0) {
                /* Call org.freedesktop.DBus.Introspectable.Introspect */
                DBusMessage *intro = dbus_message_new_method_call(
                    service, opath,
                    "org.freedesktop.DBus.Introspectable", "Introspect");
                if (intro) {
                    DBusError err;
                    dbus_error_init(&err);
                    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
                        g_dbus_conn, intro, 5000, &err);
                    dbus_message_unref(intro);
                    if (reply) {
                        char *xml = busfs_db_message_to_string(reply);
                        dbus_message_unref(reply);
                        if (xml) {
                            n = snprintf(result_buf, sizeof(result_buf), "%s", xml);
                            free(xml);
                            goto out;
                        }
                    } else {
                        n = snprintf(result_buf, sizeof(result_buf),
                            "<!-- Introspect failed: %s -->\n",
                            err.message ? err.message : "unknown");
                        dbus_error_free(&err);
                        goto out;
                    }
                }
            }
            else if (strcmp(file, "properties") == 0) {
                /* Use Introspect for now — properties via GetAll coming next */
                DBusError err;
                dbus_error_init(&err);
                DBusMessage *intro = dbus_message_new_method_call(
                    service, opath,
                    "org.freedesktop.DBus.Introspectable", "Introspect");
                if (intro) {
                    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
                        g_dbus_conn, intro, 5000, &err);
                    dbus_message_unref(intro);
                    if (reply) {
                        char *xml = busfs_db_message_to_string(reply);
                        dbus_message_unref(reply);
                        if (xml) {
                            n = snprintf(result_buf, sizeof(result_buf), "%s", xml);
                            free(xml);
                            goto out;
                        }
                    }
                }
                if (dbus_error_is_set(&err)) {
                    n = snprintf(result_buf, sizeof(result_buf),
                        "(error: %s)\n", err.message);
                    dbus_error_free(&err);
                } else {
                    n = snprintf(result_buf, sizeof(result_buf),
                        "(empty)\n");
                }
                goto out;
            }
            else {
                /* Generic file read — show info */
                n = snprintf(result_buf, sizeof(result_buf),
                    "stint-busfs: D-Bus service file\n"
                    "  service:  %s\n"
                    "  object:   %s\n"
                    "  file:     %s\n"
                    "  use 'cat .../introspect' for XML\n"
                    "  use 'cat .../properties' for properties\n",
                    service, opath, file);
                goto out;
            }
        }

        free(service);
        free(opath);
    }

    /* Fallback: demo/info string */
    {
        n = snprintf(result_buf, sizeof(result_buf),
            "stint-busfs: D-Bus virtual filesystem\n"
            "  path:     %s\n"
            "  dbus:     %s\n"
            "  contracts:%s\n"
            "  chain:    %zu entries\n"
            "  mode:     %s\n",
            path,
            g_dbus_conn ? "connected" : "demo (no bus)",
            g_config.enable_contracts ? "on" : "off",
            g_provenance.count,
            g_config.enable_signals ? "async" : "sync");
    }

out:
    if ((size_t)n <= (size_t)offset) return 0;
    size_t to_copy = (size_t)n - (size_t)offset;
    if (to_copy > size) to_copy = size;
    memcpy(buf, result_buf + offset, to_copy);

    contract_check("read", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "read", path, "ok", to_copy);
    return (int)to_copy;
}

/* ---- write: send D-Bus method call ---- */
static int busfs_write(const char *path, const char *buf, size_t size,
                       off_t offset, struct fuse_file_info *fi)
{
    (void)fi; (void)offset;
    contract_check("write", path, CONTRACT_PRE);

    if (g_dbus_conn && size > 0 && path_service(path)) {
        char *service = extract_service(path);
        char *file = (char *)path_file_part(path);
        char *opath = service_to_object_path(service);

        if (service && file && opath) {
            if (strcmp(file, "methods") == 0) {
                /* Parse method name from write payload */
                char method[256] = {0};
                char argstr[4096] = {0};
                sscanf(buf, "%255s %4095[^\n]", method, argstr);

                if (method[0]) {
                    DBusError err;
                    dbus_error_init(&err);
                    DBusMessage *call = dbus_message_new_method_call(
                        service, opath, service, method);
                    if (call) {
                        DBusMessage *reply = dbus_connection_send_with_reply_and_block(
                            g_dbus_conn, call, 5000, &err);
                        dbus_message_unref(call);
                        if (reply) dbus_message_unref(reply);
                        dbus_error_free(&err);
                    }
                }
            }
        }

        free(service);
        free(opath);
    }

    contract_check("write", path, CONTRACT_POST);
    if (g_config.enable_provenance)
        busfs_provenance_log(&g_provenance, "write", path, "ok", size);
    return (int)size;
}

/* ---- statfs ---- */
static int busfs_statfs(const char *path, struct statvfs *stbuf)
{
    (void)path;
    memset(stbuf, 0, sizeof(*stbuf));
    stbuf->f_bsize   = 4096;
    stbuf->f_frsize  = 4096;
    stbuf->f_blocks  = 1UL << 20;
    stbuf->f_bfree   = 1UL << 20;
    stbuf->f_bavail  = 1UL << 20;
    stbuf->f_files   = 1000000;
    stbuf->f_ffree   = 1000000;
    stbuf->f_namemax = 4096;
    stbuf->f_flag    = ST_NOSUID;
    return 0;
}

/* ---- release / flush ---- */
static int busfs_release(const char *path, struct fuse_file_info *fi) {
    (void)fi; (void)path; return 0;
}
static int busfs_flush(const char *path, struct fuse_file_info *fi) {
    (void)fi; (void)path; return 0;
}

/* ---- FUSE operations table ---- */
static const struct fuse_operations busfs_oper = {
    .getattr  = busfs_getattr,
    .readdir  = busfs_readdir,
    .open     = busfs_open,
    .create   = busfs_create,
    .mkdir    = busfs_mkdir,
    .read     = busfs_read,
    .write    = busfs_write,
    .statfs   = busfs_statfs,
    .flush    = busfs_flush,
    .release  = busfs_release,
    .init     = busfs_init,
    .destroy  = busfs_destroy,
};

/* ==================================================================
 * MAIN
 * ================================================================== */
int main(int argc, char *argv[])
{
    struct fuse_args args = FUSE_ARGS_INIT(argc, argv);
    struct fuse_cmdline_opts opts;
    struct fuse_loop_config loop_cfg;
    struct fuse *fuse = NULL;
    int ret = 1;

    printf("stint-busfs — D-Bus as Virtual Filesystem\n");
    printf("  DSO contracts + SHA-256 hash-chain provenance\n\n");

    /* ---- parse FUSE command line ---- */
    memset(&opts, 0, sizeof(opts));
    if (fuse_parse_cmdline(&args, &opts) != 0) {
        fprintf(stderr, "stint-busfs: failed to parse FUSE options\n");
        goto out;
    }

    if (opts.show_help) {
        printf(
            "Usage: stint-busfs <mountpoint> [options]\n\n"
            "Mount D-Bus as a virtual filesystem.\n\n"
            "FUSE options:\n"
            "  -o allow_other        allow other users to access\n"
            "  -o allow_root         allow root to access\n"
            "  -d   -o debug         enable debug output\n"
            "  -f   -o foreground    foreground mode\n"
            "  -s   -o singlethread  single-threaded mode\n"
            "\n"
            "stint-busfs options:\n"
            "  --system              connect to system bus (default: session)\n"
            "  --no-contracts        disable DSO contracts\n"
            "  --no-provenance       disable provenance chain\n"
            "\n");
        ret = 0;
        goto out;
    }

    if (opts.show_version) {
        printf("stint-busfs version 0.1 (FUSE3, async)\n");
        ret = 0;
        goto out;
    }

    /* ---- config ---- */
    g_config.bus_type           = BUSFS_SESSION;
    g_config.mount_point        = opts.mountpoint ? opts.mountpoint : "/bus";
    g_config.enable_provenance  = true;
    g_config.enable_contracts   = true;
    g_config.enable_signals     = true;
    g_config.max_read_size      = 64 * 1024;
    g_config.max_write_size     = 64 * 1024;
    g_config.contract_timeout_ms = 5000;

    for (int i = 0; i < args.argc; i++) {
        if      (strcmp(args.argv[i], "--system")        == 0) g_config.bus_type = BUSFS_SYSTEM;
        else if (strcmp(args.argv[i], "--no-contracts")  == 0) g_config.enable_contracts = false;
        else if (strcmp(args.argv[i], "--no-provenance") == 0) g_config.enable_provenance = false;
    }

    /* ---- connect D-Bus ---- */
    g_dbus_conn = busfs_db_connect(g_config.bus_type, NULL);
    if (g_dbus_conn) {
        printf("  D-Bus %s bus: CONNECTED\n",
               g_config.bus_type == BUSFS_SYSTEM ? "system" : "session");
    } else {
        fprintf(stderr, "  D-Bus: NOT AVAILABLE — running in demo mode\n");
    }

    /* ---- init provenance chain ---- */
    if (!busfs_provenance_init(&g_provenance, 1000)) {
        fprintf(stderr, "stint-busfs: provenance init failed\n");
        goto out;
    }
    printf("  Provenance chain: initialized (capacity=%zu)\n",
           g_provenance.capacity);
    printf("  DSO contracts: %s\n",
           g_config.enable_contracts ? "ENABLED" : "disabled");
    printf("\n");

    /* ---- loop config ---- */
    memset(&loop_cfg, 0, sizeof(loop_cfg));
    loop_cfg.clone_fd         = opts.clone_fd ? opts.clone_fd : 1;
    loop_cfg.max_idle_threads = opts.max_idle_threads ? opts.max_idle_threads : 8;

    /* ---- create FUSE handle ---- */
    fuse = fuse_new(&args, &busfs_oper, sizeof(busfs_oper), NULL);
    if (!fuse) {
        fprintf(stderr, "stint-busfs: fuse_new() failed\n");
        goto out;
    }

    /* ---- mount ---- */
    if (fuse_mount(fuse, opts.mountpoint) != 0) {
        fprintf(stderr, "stint-busfs: fuse_mount(%s) failed\n",
                opts.mountpoint);
        goto out;
    }
    printf("  Mount point: %s\n\n", opts.mountpoint);
    printf("  Try:  ls %s/session\n", opts.mountpoint);
    printf("  Try:  cat %s/session/org.freedesktop.UPower/introspect\n", opts.mountpoint);
    printf("  Try:  cat %s/session/org.freedesktop.DBus/properties\n", opts.mountpoint);
    printf("\n  PID: %d  Hit Ctrl-C to unmount\n", getpid());
    printf("──────────────────────────────────────────────────────\n");

    /* ---- run event loop ---- */
    if (opts.singlethread)
        ret = fuse_loop(fuse);
    else
        ret = fuse_loop_mt(fuse, &loop_cfg);

    /* ---- cleanup ---- */
    fuse_unmount(fuse);
    fuse_destroy(fuse);
    ret = 0;

out:
    if (fuse && ret != 0) {
        fuse_unmount(fuse);
        fuse_destroy(fuse);
    }
    fuse_opt_free_args(&args);
    return ret;
}
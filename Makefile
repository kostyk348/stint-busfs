CC = gcc
CFLAGS = -Wall -Wextra -O2 -DFUSE_USE_VERSION=32 -I$(CURDIR)/include $(shell pkg-config --cflags fuse3 dbus-1 openssl)
LDFLAGS = $(shell pkg-config --libs fuse3 dbus-1 openssl) -lpthread

PROG = stint-busfs
SRCS = src/busfs_db.c src/busfs_contract.c src/busfs_provenance.c src/busfs_main.c
OBJS = $(SRCS:.c=.o)

.PHONY: all clean

all: $(PROG)

$(PROG): $(OBJS)
	$(CC) -o $@ $^ $(LDFLAGS)

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJS) $(PROG)

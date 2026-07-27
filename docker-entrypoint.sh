#!/bin/sh
# Runs as root so it can fix up /data's ownership regardless of who owns it
# on the host (bind mounts and freshly-created named volumes are commonly
# root-owned, or owned by a host UID that doesn't match the container's
# unprivileged user) -- then drops privileges before ever running app code.
set -e

mkdir -p /data
chown -R linkrs:linkrs /data

# setpriv replaces this process directly (no fork), so linkrs stays PID 1
# and still receives SIGTERM/SIGINT directly for graceful shutdown.
exec setpriv --reuid=linkrs --regid=linkrs --init-groups /usr/local/bin/linkrs

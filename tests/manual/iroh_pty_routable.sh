#!/bin/bash
# RAI-305: run the PTY lane acceptance harness across three routable hosts.
#
# Three network stacks joined by a forwarding router, so the client reaches the
# directory and the relay without being able to reach the daemon directly:
#
#   client 10.41.0.1 -- 10.41.0.2 router 10.42.0.1 -- 10.42.0.2 daemon
#
# The relay and the directory live on the router, which is where a relay really
# sits: both ends can reach it, and it can reach the daemon when the client
# cannot. That is what makes the relay fallback case mean something.
set -euo pipefail

BIN=$1
SCRATCH=$2

exec unshare -Urn bash -c '
set -euo pipefail
BIN=$1; SCRATCH=$2
CLIENT_IP=10.41.0.1; ROUTER_A=10.41.0.2; ROUTER_B=10.42.0.1; DAEMON_IP=10.42.0.2

unshare -n sleep 3600 &
ROUTER=$!
unshare -n sleep 3600 &
DAEMON=$!
cleanup() {
  kill "$ROUTER" "$DAEMON" 2>/dev/null || true
  wait "$ROUTER" 2>/dev/null || true
  wait "$DAEMON" 2>/dev/null || true
}
trap cleanup EXIT

ip link add ca type veth peer name rc
ip link add rd type veth peer name db
ip link set rc netns "$ROUTER"
ip link set rd netns "$ROUTER"
ip link set db netns "$DAEMON"
ip addr add "$CLIENT_IP/24" dev ca
ip link set ca up
ip link set lo up
nsenter -t "$ROUTER" -n ip addr add "$ROUTER_A/24" dev rc
nsenter -t "$ROUTER" -n ip addr add "$ROUTER_B/24" dev rd
nsenter -t "$ROUTER" -n ip link set rc up
nsenter -t "$ROUTER" -n ip link set rd up
nsenter -t "$ROUTER" -n ip link set lo up
nsenter -t "$ROUTER" -n sysctl -qw net.ipv4.ip_forward=1
nsenter -t "$DAEMON" -n ip addr add "$DAEMON_IP/24" dev db
nsenter -t "$DAEMON" -n ip link set db up
nsenter -t "$DAEMON" -n ip link set lo up
nsenter -t "$DAEMON" -n ip route add 10.41.0.0/24 via "$ROUTER_B"
ip route add 10.42.0.0/24 via "$ROUTER_A"

echo "topology: client=$CLIENT_IP router=$ROUTER_A/$ROUTER_B daemon=$DAEMON_IP"
ping -c1 -W2 "$ROUTER_B" >/dev/null || { echo "no route to the relay"; exit 1; }
ping -c1 -W2 "$DAEMON_IP" >/dev/null || { echo "no route to the daemon"; exit 1; }
nsenter -t "$DAEMON" -n ping -c1 -W2 "$ROUTER_B" >/dev/null || { echo "daemon cannot reach the relay"; exit 1; }

RAI305_RELAY_NS=$ROUTER RAI305_DAEMON_NS=$DAEMON \
  python3 "$SCRATCH/iroh_pty_lane.py" "$BIN" "$SCRATCH/scratch" \
    --routable "$ROUTER_B" "$DAEMON_IP" "$CLIENT_IP"
' _ "$BIN" "$SCRATCH"

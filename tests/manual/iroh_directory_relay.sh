#!/bin/bash
# RAI-426: reach a directory that has no inbound address.
#
# Three network stacks, like iroh_pty_routable.sh, but the directory endpoint is
# bound on the router's own loopback: no peer can dial it by IP, which is exactly
# the state a directory behind NAT is in. Only the relay is mutually reachable,
# so a lookup that succeeds there was carried by the relay and nothing else.
#
#   client 10.51.0.1 -- 10.51.0.2 router 10.52.0.1 -- 10.52.0.2 daemon
#
# The first case keeps today's profile shape (an IP address for the directory)
# and has to fail: without the switch there is no path to the directory at all.
set -euo pipefail

BIN=$1
SCRATCH=$2

exec unshare -Urn bash -c '
set -euo pipefail
BIN=$1; SCRATCH=$2
CLIENT_IP=10.51.0.1; ROUTER_A=10.51.0.2; ROUTER_B=10.52.0.1; DAEMON_IP=10.52.0.2

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
nsenter -t "$DAEMON" -n ip route add 10.51.0.0/24 via "$ROUTER_B"
ip route add 10.52.0.0/24 via "$ROUTER_A"

echo "topology: client=$CLIENT_IP router=$ROUTER_A/$ROUTER_B daemon=$DAEMON_IP directory=127.0.0.1@router"
ping -c1 -W2 "$ROUTER_A" >/dev/null || { echo "no route to the relay"; exit 1; }
nsenter -t "$DAEMON" -n ping -c1 -W2 "$ROUTER_A" >/dev/null || { echo "daemon cannot reach the relay"; exit 1; }

RAI426_RELAY_NS=$ROUTER RAI426_DAEMON_NS=$DAEMON \
  python3 "$SCRATCH/iroh_directory_relay.py" "$BIN" "$SCRATCH/scratch" \
    "$ROUTER_A" "$DAEMON_IP" "$CLIENT_IP"
' _ "$BIN" "$SCRATCH"

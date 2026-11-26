#!/bin/bash
# RAI-305: run the advertise lane across an emulated 1:1 NAT.
#
# Three stacks, because that is what a 1:1 NAT looks like from the software's
# side: the relay holds a private address, the NAT device owns the public one and
# still forwards every port 1:1 to the private address, and the peers know only
# the public address. The relay therefore cannot bind the address peers are told
# to dial, which is exactly the case `advertise` exists for.
#
#   peer 10.60.0.2 -- 10.60.0.1 nat 10.50.0.254 -- 10.50.0.2 relay
set -euo pipefail

BIN=$1
SCRATCH=$2
DIR_PORT=${RAI305_ADV_DIR_PORT:-45401}
HTTP_PORT=${RAI305_ADV_HTTP_PORT:-45402}
HTTPS_PORT=${RAI305_ADV_HTTPS_PORT:-45403}
QAD_PORT=${RAI305_ADV_QAD_PORT:-45404}

exec unshare -Urn bash -c '
set -euo pipefail
BIN=$1; SCRATCH=$2
DIR_PORT=$3; HTTP_PORT=$4; HTTPS_PORT=$5; QAD_PORT=$6
PEER_IP=10.60.0.2; NAT_PUBLIC=10.60.0.1; NAT_PRIVATE=10.50.0.254; RELAY_IP=10.50.0.2

unshare -n sleep 3600 &
NAT=$!
unshare -n sleep 3600 &
RELAY=$!
cleanup() {
  kill "$NAT" "$RELAY" 2>/dev/null || true
  wait "$NAT" 2>/dev/null || true
  wait "$RELAY" 2>/dev/null || true
}
trap cleanup EXIT

ip link add pa type veth peer name na
ip link add nr type veth peer name rn
ip link set na netns "$NAT"
ip link set nr netns "$NAT"
ip link set rn netns "$RELAY"

ip addr add "$PEER_IP/24" dev pa
ip link set pa up
ip link set lo up
ip route add default via "$NAT_PUBLIC"

nsenter -t "$NAT" -n ip addr add "$NAT_PUBLIC/24" dev na
nsenter -t "$NAT" -n ip addr add "$NAT_PRIVATE/24" dev nr
nsenter -t "$NAT" -n ip link set na up
nsenter -t "$NAT" -n ip link set nr up
nsenter -t "$NAT" -n ip link set lo up
nsenter -t "$NAT" -n sysctl -qw net.ipv4.ip_forward=1

nsenter -t "$RELAY" -n ip addr add "$RELAY_IP/24" dev rn
nsenter -t "$RELAY" -n ip link set rn up
nsenter -t "$RELAY" -n ip link set lo up
nsenter -t "$RELAY" -n ip route add default via "$NAT_PRIVATE"

# 1:1 NAT: every port peers might dial on the public address forwards to the
# same port on the private one, and replies come back through the NAT device.
nsenter -t "$NAT" -n iptables -t nat -A PREROUTING -d "$NAT_PUBLIC" -p udp --dport "$DIR_PORT"   -j DNAT --to-destination "$RELAY_IP"
nsenter -t "$NAT" -n iptables -t nat -A PREROUTING -d "$NAT_PUBLIC" -p tcp --dport "$HTTP_PORT"  -j DNAT --to-destination "$RELAY_IP"
nsenter -t "$NAT" -n iptables -t nat -A PREROUTING -d "$NAT_PUBLIC" -p tcp --dport "$HTTPS_PORT" -j DNAT --to-destination "$RELAY_IP"
nsenter -t "$NAT" -n iptables -t nat -A PREROUTING -d "$NAT_PUBLIC" -p udp --dport "$QAD_PORT"   -j DNAT --to-destination "$RELAY_IP"
nsenter -t "$NAT" -n iptables -t nat -A POSTROUTING -s 10.60.0.0/24 -o nr -j SNAT --to-source "$NAT_PRIVATE"

echo "topology: peer=$PEER_IP public=$NAT_PUBLIC private=$RELAY_IP ports=$DIR_PORT/$HTTP_PORT/$HTTPS_PORT/$QAD_PORT"
ping -c1 -W2 "$NAT_PUBLIC" >/dev/null || { echo "no route to the public address"; exit 1; }
nsenter -t "$RELAY" -n ping -c1 -W2 "$NAT_PRIVATE" >/dev/null || { echo "relay cannot reach the NAT device"; exit 1; }

RAI305_ADV_RELAY_IP=$RELAY_IP RAI305_ADV_PUBLIC_IP=$NAT_PUBLIC \
RAI305_ADV_PEER_IP=$PEER_IP RAI305_ADV_PORTS=$DIR_PORT,$HTTP_PORT,$HTTPS_PORT,$QAD_PORT \
RAI305_ADV_RELAY_NS=$RELAY \
  python3 "$SCRATCH/iroh_pty_lane.py" "$BIN" "$SCRATCH/scratch" --advertise
' _ "$BIN" "$SCRATCH" "$DIR_PORT" "$HTTP_PORT" "$HTTPS_PORT" "$QAD_PORT"

#!/usr/bin/env bash
#
# Ordering guard.
#
# Constraint 1 of docs/postmortem.md: no transport work until several protocols run against the
# in-memory simulator. The previous attempt rewrote the connection manager four times across
# three framework bets and never reached the algorithms. This makes the constraint a build
# failure rather than a resolution.
#
# When transport genuinely arrives — constraint 5, and a change of its own — delete this script
# deliberately, in the commit that introduces it.
#
# Usage: ./scripts/check-no-transport.sh

set -uo pipefail
cd "$(dirname "$0")/.."

fail=0

banned_crates='^(tokio|async-std|smol|futures|futures-core|mio|socket2|quinn|hyper|actix|tower|reqwest|rustls|polling|calloop)([ -]|$)'
hits=$(cargo tree --workspace -e normal,build,dev --prefix none 2>/dev/null | sort -u | grep -Ei "$banned_crates" || true)
if [ -n "$hits" ]; then
    echo "FAIL: an async runtime or networking crate is in the dependency tree:"
    echo "$hits"
    fail=1
fi

banned_source='\b(TcpStream|TcpListener|UdpSocket|SocketAddr|std::net|tokio|async fn|\.await)\b'
hits=$(grep -rnE "$banned_source" crates --include='*.rs' || true)
if [ -n "$hits" ]; then
    echo "FAIL: transport or async constructs in the protocol crates:"
    echo "$hits"
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo ""
    echo "Protocols are sans-IO and the simulator is the network. If this needs to change,"
    echo "it needs a change proposal, not an exception."
    exit 1
fi

echo "PASS: no transport, no async runtime, no sockets"

# Vendored librqbit dual-stack sockets

Upstream: `https://github.com/ikatson/librqbit-dualstack-sockets`

Upstream crate: `librqbit-dualstack-sockets` 0.7.0

## Why this fork exists

Windows reports ICMP UDP `PORT_UNREACHABLE` and `NET_UNREACHABLE` responses as
`WSAECONNRESET` or `WSAENETRESET` on a later receive by default. Public DHT and
uTP traffic routinely encounters stale or unreachable nodes, so these expected
network responses terminated rqbit's shared receive loops and prevented
subsequent metadata discovery or piece transfer.

Every UDP socket disables these reports with Microsoft's `SIO_UDP_CONNRESET`
and `SIO_UDP_NETRESET` controls before it is bound or handed to Tokio.
Individual unreachable nodes remain ordinary unanswered requests; the shared
DHT and uTP dispatchers stay alive. The Windows-only regression test sends to a
closed local UDP port and then verifies that a valid datagram can still be
received.

## Maintenance invariant

Keep this fork aligned with 0.7.0 except for the documented Windows socket
control and its test. Remove the patch after a qualified upstream release has
equivalent behavior.

## Tradeoff

The patch contains one small Windows FFI call through `windows-sys`. Failing to
apply the control is treated as socket initialization failure rather than
silently running a DHT receiver known to terminate under normal network input.

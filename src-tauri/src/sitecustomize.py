"""Loaded by every Python program the Android app runs -- in practice yt-dlp.

Android looks host names up for an app through the phone's resolver. When that
resolver gives no answer -- a VPN or Private DNS server that is down, or a
filter that refuses a name -- the lookup fails with "No address associated
with hostname" although the connection itself would work. Such a lookup is
then asked of a public DNS-over-HTTPS resolver instead. The query goes over
HTTPS to an IP address, so it needs no DNS of its own, and it carries only the
host name. While the phone's resolver answers, nothing here does anything.

The app writes this file next to the bundled interpreter and names its folder
in PYTHONPATH (see `android::command`). Python reports an error raised while
loading it and carries on, and nothing in the fallback raises anything but the
lookup error the program would have seen anyway.
"""

import os
import socket
import time

# Asked in parallel, and the first answer is used. Two operators, so that one
# being unreachable from a network does not stop the fallback.
_RESOLVERS = (
    ("1.1.1.1", "/dns-query"),
    ("8.8.8.8", "/resolve"),
)

# Seconds a resolver has to answer. A network that silently drops the app's
# traffic would otherwise hold the lookup for as long as the socket allows.
_TIMEOUT = 3.0

_A = 1
_AAAA = 28

# The errors that mean the resolver found no address, as opposed to a call
# that was malformed.
_LOOKUP_FAILURES = {
    getattr(socket, name)
    for name in ("EAI_AGAIN", "EAI_FAIL", "EAI_NODATA", "EAI_NONAME")
    if hasattr(socket, name)
}

_system_getaddrinfo = socket.getaddrinfo

# name, record type -> (expires, addresses)
_answers = {}

# Once the phone's resolver has failed where the fallback did not, it is
# skipped rather than waited on: a resolver that does not answer can take
# seconds to say so, for every host. The app says the same when its own
# lookups have been failing.
_prefer_fallback = os.environ.get("UD_DNS_FALLBACK_FIRST") == "1"


def getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
    global _prefer_fallback

    if not _is_name(host, flags):
        return _system_getaddrinfo(host, port, family, type, proto, flags)

    if _prefer_fallback:
        found, _ = _fallback(host, port, family, type, proto, flags)
        if found:
            return found

    try:
        return _system_getaddrinfo(host, port, family, type, proto, flags)
    except socket.gaierror as error:
        if error.errno not in _LOOKUP_FAILURES:
            raise
        found, reason = _fallback(host, port, family, type, proto, flags)
        if not found:
            raise socket.gaierror(error.errno, f"{error.strerror}; backup lookup: {reason}") from None
        _prefer_fallback = True
        return found


def _is_name(host, flags):
    """Whether `host` is a name a public resolver could know. An address needs
    no lookup, and a name only the local network knows -- `localhost`, a
    printer, anything under `.local` -- is not sent out."""
    if not isinstance(host, str) or flags & socket.AI_NUMERICHOST:
        return False
    name = host.rstrip(".").lower()
    if "." not in name or name.endswith((".localhost", ".local")):
        return False
    return _address_family(name) is None


def _address_family(text):
    for family in (socket.AF_INET, socket.AF_INET6):
        try:
            socket.inet_pton(family, text.split("%", 1)[0])
            return family
        except (OSError, ValueError):
            pass
    return None


def _fallback(host, port, family, type, proto, flags):
    """getaddrinfo's answer built from the public resolvers' addresses, or
    an empty list and the reason there is none."""
    try:
        name = host.rstrip(".").lower().encode("idna").decode("ascii")
    except UnicodeError:
        return [], "not a valid host name"

    # IPv6 addresses are only asked for when there are no IPv4 ones, and not
    # at all when the resolvers could not be reached for those.
    kinds = {socket.AF_INET: (_A,), socket.AF_INET6: (_AAAA,)}.get(family, (_A, _AAAA))
    for kind in kinds:
        addresses, failure = _addresses(name, kind)
        if failure:
            return [], failure
        found = []
        for address in addresses:
            try:
                found += _system_getaddrinfo(
                    address, port, family, type, proto, flags | socket.AI_NUMERICHOST
                )
            except OSError:
                pass
        if found:
            return found, None
    return [], "no address"


def _addresses(name, kind):
    cached = _answers.get((name, kind))
    if cached and cached[0] > time.monotonic():
        return cached[1], None
    answer, failure = _ask_resolvers(name, kind)
    if answer is None:
        return [], failure
    addresses, ttl = answer
    _answers[(name, kind)] = (time.monotonic() + ttl, addresses)
    return addresses, None


def _ask_resolvers(name, kind):
    import queue
    import threading

    replies = queue.Queue()
    for resolver in _RESOLVERS:
        # Daemon threads: a resolver that never answers must not keep the
        # program from exiting once it is done.
        threading.Thread(target=_ask_into, args=(replies, resolver, name, kind), daemon=True).start()

    failures = []
    deadline = time.monotonic() + _TIMEOUT + 0.5
    for _ in _RESOLVERS:
        try:
            answered, value = replies.get(timeout=max(0.0, deadline - time.monotonic()))
        except queue.Empty:
            failures.append("no answer in time")
            break
        if answered:
            return value, None
        failures.append(value)
    return None, ", ".join(failures)


def _ask_into(replies, resolver, name, kind):
    address, path = resolver
    try:
        replies.put((True, _ask(address, path, name, kind)))
    except Exception as error:
        replies.put((False, f"{address} {error or type(error).__name__}"))


def _ask(address, path, name, kind):
    import http.client
    import json
    import ssl
    from urllib.parse import quote

    # The certificates of both services name their IP addresses, so the
    # connection is verified like any other.
    connection = http.client.HTTPSConnection(
        address, 443, timeout=_TIMEOUT, context=ssl.create_default_context()
    )
    try:
        connection.request(
            "GET",
            f"{path}?name={quote(name)}&type={kind}",
            headers={"Accept": "application/dns-json"},
        )
        response = connection.getresponse()
        if response.status != 200:
            raise OSError(f"HTTP {response.status}")
        return parse_answer(json.loads(response.read(1 << 16)), kind)
    finally:
        connection.close()


def parse_answer(body, kind):
    """The addresses of `kind` in a JSON DNS answer and how long they may be
    kept. A name that does not exist is an answer too: no addresses."""
    status = body.get("Status")
    if status == 3:
        return [], 300
    if status != 0:
        raise OSError(f"DNS status {status}")

    family = socket.AF_INET if kind == _A else socket.AF_INET6
    addresses = []
    ttl = 600
    for record in body.get("Answer") or ():
        data = record.get("data")
        if record.get("type") == kind and isinstance(data, str) and _address_family(data) == family:
            addresses.append(data)
            ttl = min(ttl, int(record.get("TTL", 60)))
    return addresses, max(30, ttl)


socket.getaddrinfo = getaddrinfo

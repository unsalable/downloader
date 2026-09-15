"""Tests for the DNS fallback the Android app loads into the engine's Python.

Run with `npm run test:python`. The lookups are replaced by stand-ins, so no
network is needed, except for the one test that asks the real resolvers when
UD_ONLINE_TESTS=1 is set.
"""

import importlib.util
import os
import socket
import sys
import unittest
from pathlib import Path

HOOK = Path(__file__).resolve().parents[2] / "src" / "sitecustomize.py"

# The module lives among the Rust sources; no bytecode cache is left there.
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location("ud_sitecustomize", HOOK)
hook = importlib.util.module_from_spec(_spec)
_real_getaddrinfo = socket.getaddrinfo
_spec.loader.exec_module(hook)
# Loading it installs it; the tests call it directly instead.
socket.getaddrinfo = _real_getaddrinfo

NO_ADDRESS = next(iter(hook._LOOKUP_FAILURES))


def failing_system(asked):
    """A resolver that answers addresses and nothing else, like a phone whose
    DNS server does not respond."""

    def getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
        asked.append(host)
        if flags & socket.AI_NUMERICHOST or hook._address_family(host):
            return _real_getaddrinfo(host, port, family, type, proto, flags | socket.AI_NUMERICHOST)
        raise socket.gaierror(NO_ADDRESS, "No address associated with hostname")

    return getaddrinfo


class DnsFallbackTest(unittest.TestCase):
    def setUp(self):
        self.asked_system = []
        self.asked_public = []
        self.public_answer = (["2.20.134.193"], 60)
        self.public_failure = None

        def ask_resolvers(name, kind):
            self.asked_public.append((name, kind))
            if self.public_failure:
                return None, self.public_failure
            return (self.public_answer if kind == hook._A else ([], 60)), None

        self.saved = (hook._system_getaddrinfo, hook._ask_resolvers, hook._prefer_fallback)
        hook._system_getaddrinfo = failing_system(self.asked_system)
        hook._ask_resolvers = ask_resolvers
        hook._prefer_fallback = False
        hook._answers.clear()

    def tearDown(self):
        hook._system_getaddrinfo, hook._ask_resolvers, hook._prefer_fallback = self.saved
        hook._answers.clear()

    def test_a_public_resolver_answers_when_the_phone_does_not(self):
        found = hook.getaddrinfo("vt.tiktok.com", 443, 0, socket.SOCK_STREAM)

        self.assertEqual({entry[4][:2] for entry in found}, {("2.20.134.193", 443)})
        self.assertEqual(found[0][1], socket.SOCK_STREAM)
        self.assertEqual(self.asked_public, [("vt.tiktok.com", hook._A)])
        self.assertTrue(hook._prefer_fallback)

    def test_once_the_phone_has_failed_it_is_not_waited_on_again(self):
        hook.getaddrinfo("vt.tiktok.com", 443)
        self.asked_system.clear()

        hook.getaddrinfo("www.tiktok.com", 443)

        self.assertNotIn("www.tiktok.com", self.asked_system)
        self.assertEqual(self.asked_public[-1], ("www.tiktok.com", hook._A))

    def test_an_answer_is_kept_for_the_next_lookup(self):
        hook.getaddrinfo("vt.tiktok.com", 443)
        hook.getaddrinfo("vt.tiktok.com", 80)
        self.assertEqual(self.asked_public, [("vt.tiktok.com", hook._A)])

    def test_when_both_fail_the_error_says_why_and_keeps_its_number(self):
        self.public_failure = "1.1.1.1 timed out, 8.8.8.8 timed out"

        with self.assertRaises(socket.gaierror) as raised:
            hook.getaddrinfo("vt.tiktok.com", 443)

        self.assertEqual(raised.exception.errno, NO_ADDRESS)
        self.assertEqual(
            str(raised.exception),
            f"[Errno {NO_ADDRESS}] No address associated with hostname; "
            "backup lookup: 1.1.1.1 timed out, 8.8.8.8 timed out",
        )
        self.assertFalse(hook._prefer_fallback)

    def test_ipv6_is_asked_for_only_when_there_is_no_ipv4(self):
        self.public_answer = ([], 60)
        with self.assertRaises(socket.gaierror):
            hook.getaddrinfo("v6only.example", 443)
        self.assertEqual(self.asked_public, [("v6only.example", hook._A), ("v6only.example", hook._AAAA)])

        self.asked_public.clear()
        self.public_failure = "1.1.1.1 timed out, 8.8.8.8 timed out"
        with self.assertRaises(socket.gaierror):
            hook.getaddrinfo("unreachable.example", 443)
        self.assertEqual(self.asked_public, [("unreachable.example", hook._A)])

    def test_local_names_and_addresses_never_leave_the_phone(self):
        for host in ("localhost", "printer", "nas.local", "app.localhost"):
            with self.assertRaises(socket.gaierror):
                hook.getaddrinfo(host, 80)
        hook.getaddrinfo("192.168.1.20", 80)
        self.assertEqual(self.asked_public, [])

    def test_a_malformed_call_is_not_mistaken_for_a_failed_lookup(self):
        def refuse(*args, **kwargs):
            raise socket.gaierror(socket.EAI_SERVICE, "Servname not supported for ai_socktype")

        hook._system_getaddrinfo = refuse
        with self.assertRaises(socket.gaierror) as raised:
            hook.getaddrinfo("vt.tiktok.com", "no-such-service")
        self.assertEqual(raised.exception.errno, socket.EAI_SERVICE)
        self.assertEqual(self.asked_public, [])

    def test_the_phones_answer_is_used_whenever_it_has_one(self):
        def answering(host, port, family=0, type=0, proto=0, flags=0):
            return _real_getaddrinfo("127.0.0.1", port, family, type, proto, flags)

        hook._system_getaddrinfo = answering
        found = hook.getaddrinfo("vt.tiktok.com", 80)
        self.assertEqual(found[0][4][:2], ("127.0.0.1", 80))
        self.assertEqual(self.asked_public, [])


class AnswerTest(unittest.TestCase):
    def test_the_addresses_an_alias_leads_to_are_taken(self):
        # What 1.1.1.1 answered for a TikTok short link.
        body = {
            "Status": 0,
            "Answer": [
                {"name": "vt.tiktok.com", "type": 5, "TTL": 556, "data": "vt.tiktok.com.edgesuite.net."},
                {"name": "a1801.r.akamai.net", "type": 1, "TTL": 120, "data": "2.20.134.193"},
                {"name": "a1801.r.akamai.net", "type": 1, "TTL": 90, "data": "2.19.193.193"},
            ],
        }
        self.assertEqual(hook.parse_answer(body, hook._A), (["2.20.134.193", "2.19.193.193"], 90))

    def test_records_of_the_other_family_are_not_taken(self):
        body = {
            "Status": 0,
            "Answer": [
                {"type": 28, "TTL": 5, "data": "2606:4700::6810:84e5"},
                {"type": 28, "TTL": 5, "data": "104.16.132.229"},
            ],
        }
        self.assertEqual(hook.parse_answer(body, hook._AAAA), (["2606:4700::6810:84e5"], 30))
        self.assertEqual(hook.parse_answer(body, hook._A)[0], [])

    def test_a_name_that_does_not_exist_is_an_answer_and_a_failure_is_not(self):
        self.assertEqual(hook.parse_answer({"Status": 3}, hook._A)[0], [])
        with self.assertRaises(OSError):
            hook.parse_answer({"Status": 2}, hook._A)


@unittest.skipUnless(os.environ.get("UD_ONLINE_TESTS") == "1", "needs the internet; set UD_ONLINE_TESTS=1")
class OnlineTest(unittest.TestCase):
    def test_the_public_resolvers_find_a_real_host(self):
        answer, failure = hook._ask_resolvers("vt.tiktok.com", hook._A)
        self.assertIsNone(failure)
        addresses, ttl = answer
        self.assertTrue(addresses)
        self.assertGreaterEqual(ttl, 30)


if __name__ == "__main__":
    unittest.main()

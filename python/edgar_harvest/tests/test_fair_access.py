import gzip
import unittest
from argparse import Namespace
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import mock

from edgar_harvest.cli import (
    DEFAULT_OUTPUT,
    DEFAULT_QUERIES,
    DEFAULT_SLEEP_S,
    DEFAULT_START,
    MIN_SLEEP_S,
    build_parser,
    decode_body,
    fetch,
    harvest,
    placeholder_user_agent,
    refuse_allowlist_output,
    require_network_user_agent,
    require_sleep,
)


class PlaceholderUserAgentTests(unittest.TestCase):
    def test_empty_and_example_com_are_placeholders(self):
        self.assertTrue(placeholder_user_agent(""))
        self.assertTrue(placeholder_user_agent("  "))
        self.assertTrue(placeholder_user_agent("tail-to-ticker-edgar-harvest/0.1 (contact@example.com)"))
        self.assertFalse(placeholder_user_agent("tail-to-ticker you@real-domain"))

    def test_require_network_user_agent_refuses_placeholder(self):
        with self.assertRaises(SystemExit):
            require_network_user_agent("contact@example.com")
        require_network_user_agent("tail-to-ticker you@real-domain")


class DefaultArgsTests(unittest.TestCase):
    def test_defaults_are_raw_live_phrases(self):
        args = build_parser().parse_args([])
        self.assertEqual(args.output, DEFAULT_OUTPUT)
        self.assertEqual(args.output, "evidence/edgar_hits_raw.jsonl")
        self.assertEqual(args.start, DEFAULT_START)
        self.assertEqual(args.start, "2018-01-01")
        self.assertEqual(tuple(args.query), DEFAULT_QUERIES)
        self.assertNotIn('"corporate aircraft"', args.query)
        self.assertGreaterEqual(args.sleep, 0.5)

    def test_refuses_allowlist_output(self):
        with self.assertRaises(SystemExit):
            refuse_allowlist_output(Path("overrides/edgar_allowlist.jsonl"))
        refuse_allowlist_output(Path("evidence/edgar_hits_raw.jsonl"))


class SleepTests(unittest.TestCase):
    def test_default_is_well_under_ten_per_second(self):
        self.assertGreaterEqual(DEFAULT_SLEEP_S, 0.5)
        self.assertGreaterEqual(MIN_SLEEP_S, 0.1)
        args = build_parser().parse_args([])
        self.assertEqual(args.sleep, DEFAULT_SLEEP_S)

    def test_reject_sleep_that_would_exceed_ten_per_second(self):
        with self.assertRaises(SystemExit):
            require_sleep(0.09)
        require_sleep(0.1)
        require_sleep(0.5)


class DecodeBodyTests(unittest.TestCase):
    def test_gzip(self):
        payload = b'{"ok": true}'
        self.assertEqual(decode_body(gzip.compress(payload), "gzip"), payload)

    def test_identity(self):
        self.assertEqual(decode_body(b"plain", None), b"plain")


class FetchHeaderTests(unittest.TestCase):
    def test_sends_gzip_accept_encoding(self):
        captured: dict[str, str] = {}

        class FakeResp:
            headers = {"Content-Encoding": ""}

            def read(self):
                return b"{}"

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def fake_urlopen(req, timeout=60):  # noqa: ARG001
            captured["accept_encoding"] = req.get_header("Accept-encoding") or ""
            captured["accept_language"] = req.get_header("Accept-language") or ""
            captured["user_agent"] = req.get_header("User-agent") or ""
            return FakeResp()

        env = {
            "SEC_USER_AGENT": "tail-to-ticker you@real-domain"
        }
        with mock.patch.dict("os.environ", env):
            with mock.patch("edgar_harvest.cli.urllib.request.urlopen", side_effect=fake_urlopen):
                fetch("https://example.invalid/x")
        self.assertIn("gzip", captured["accept_encoding"].lower())
        self.assertIn("deflate", captured["accept_encoding"].lower())
        self.assertIn("en-US", captured["accept_language"])
        self.assertIn("tail-to-ticker", captured["user_agent"])


class HarvestGateTests(unittest.TestCase):
    def test_harvest_refuses_placeholder_ua_before_any_get(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "hits.jsonl"
            args = Namespace(
                output=str(out),
                n_numbers_file=None,
                query=["aircraft"],
                forms="10-K",
                start="2020-01-01",
                end="2026-12-31",
                max_hits=1,
                sleep=0.5,
            )
            with mock.patch.dict("os.environ", {"SEC_USER_AGENT": "contact@example.com"}):
                with mock.patch("edgar_harvest.cli.fetch") as fetch:
                    with self.assertRaises(SystemExit):
                        harvest(args)
                    fetch.assert_not_called()

    def test_harvest_refuses_too_fast_sleep_before_any_get(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "hits.jsonl"
            args = Namespace(
                output=str(out),
                n_numbers_file=None,
                query=["aircraft"],
                forms="10-K",
                start="2020-01-01",
                end="2026-12-31",
                max_hits=1,
                sleep=0.05,
            )
            env = {
                "SEC_USER_AGENT": "tail-to-ticker you@real-domain"
            }
            with mock.patch.dict("os.environ", env):
                with mock.patch("edgar_harvest.cli.fetch") as fetch:
                    with self.assertRaises(SystemExit):
                        harvest(args)
                    fetch.assert_not_called()

    def test_harvest_refuses_allowlist_path_before_any_get(self):
        args = Namespace(
            output="overrides/edgar_allowlist.jsonl",
            n_numbers_file=None,
            query=["aircraft"],
            forms="10-K",
            start="2018-01-01",
            end="2026-12-31",
            max_hits=1,
            sleep=0.5,
        )
        env = {"SEC_USER_AGENT": "tail-to-ticker you@real-domain"}
        with mock.patch.dict("os.environ", env):
            with mock.patch("edgar_harvest.cli.fetch") as fetch:
                with self.assertRaises(SystemExit):
                    harvest(args)
                fetch.assert_not_called()


if __name__ == "__main__":
    unittest.main()

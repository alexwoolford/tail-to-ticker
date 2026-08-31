import unittest

from edgar_harvest.cli import extract_hits, keyword_hit


class ExtractTests(unittest.TestCase):
    def test_n_number_with_aviation_context(self):
        text = "Personal use of the corporate aircraft N1WM and related crew costs."
        hits = extract_hits(text, None)
        self.assertEqual(hits[0][0], "N1WM")
        self.assertTrue(keyword_hit(hits[0][1]))

    def test_bare_n_number_without_keywords_dropped(self):
        text = "Bond series N1234 priced today in the primary market."
        self.assertEqual(extract_hits(text, None), [])

    def test_allowlist_filter(self):
        text = "The corporate jet N1WM and corporate jet N9ZZ both flew."
        hits = extract_hits(text, {"N1WM"})
        self.assertEqual([n for n, _ in hits], ["N1WM"])


if __name__ == "__main__":
    unittest.main()

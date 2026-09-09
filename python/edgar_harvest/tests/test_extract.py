import unittest

from edgar_harvest.cli import (
    document_url_for_sequence,
    efts_document_filename,
    extract_hits,
    hit_document_url,
    keyword_hit,
    primary_document_url,
    skip_document_filename,
)

CHEVRON_INDEX = """
<tr>
            <td scope="row">1</td>
            <td scope="row">10-Q</td>
            <td scope="row"><a href="/ix?doc=/Archives/edgar/data/93410/000009341019000014/cvx03312019-10qdoc.htm">cvx03312019-10qdoc.htm</a></td>
            <td scope="row">10-Q</td>
            <td scope="row">1742514</td>
            <td scope="row">2</td>
            <td scope="row">EXHIBIT 10.1</td>
            <td scope="row"><a href="/Archives/edgar/data/93410/000009341019000014/a03312019ex101aircraft.htm">a03312019ex101aircraft.htm</a></td>
            <td scope="row">EX-10.1</td>
            <td scope="row">102221</td>
</tr>
"""


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


class PrimaryDocumentUrlTests(unittest.TestCase):
    def test_ix_doc_beats_index_html(self):
        html = """
        <a href="/index.htm">home</a>
        <a href="/ix?doc=/Archives/edgar/data/1803914/000180391424000027/ply-20240424.htm">DEF 14A</a>
        <a href="/Archives/edgar/data/1803914/000180391424000027/0001803914-24-000027-index.html">index</a>
        """
        url = primary_document_url(html, "0001803914", "0001803914-24-000027")
        self.assertTrue(url.endswith("/ply-20240424.htm"))
        self.assertTrue(url.startswith("https://www.sec.gov/Archives/"))

    def test_archives_htm_when_no_ix(self):
        html = """
        <a href="/Archives/edgar/data/104169/000010416924000012/wmt-20240530.htm">DEF 14A</a>
        """
        url = primary_document_url(html, "0000104169", "0000104169-24-000012")
        self.assertTrue(url.endswith("/wmt-20240530.htm"))

    def test_complete_txt_fallback(self):
        url = primary_document_url("<html></html>", "0000104169", "0000104169-24-000012")
        self.assertTrue(url.endswith("/0000104169-24-000012.txt"))

    def test_ix_primary_is_not_the_efts_exhibit(self):
        url = primary_document_url(CHEVRON_INDEX, "0000093410", "0000093410-19-000014")
        self.assertTrue(url.endswith("/cvx03312019-10qdoc.htm"))


class EftsDocumentIdTests(unittest.TestCase):
    def test_id_is_accession_colon_filename(self):
        name = efts_document_filename(
            "0000093410-19-000014:a03312019ex101aircraft.htm",
            "0000093410-19-000014",
        )
        self.assertEqual(name, "a03312019ex101aircraft.htm")

    def test_skip_graphics(self):
        self.assertTrue(skip_document_filename("beo1q19.jpg"))
        self.assertIsNone(
            efts_document_filename("0000093410-19-000014:beo1q19.jpg", "0000093410-19-000014")
        )

    def test_sequence_picks_exhibit_not_primary(self):
        url = document_url_for_sequence(CHEVRON_INDEX, 2)
        self.assertIsNotNone(url)
        self.assertTrue(url.endswith("/a03312019ex101aircraft.htm"))
        self.assertFalse(url.endswith("/cvx03312019-10qdoc.htm"))

    def test_hit_document_url_uses_id_without_index_get(self):
        hit = {"_id": "0000093410-19-000014:a03312019ex101aircraft.htm"}
        src = {"sequence": 2, "file_type": "EX-10.1"}
        url = hit_document_url("0000093410", "0000093410-19-000014", hit, src, sleep=0)
        self.assertTrue(url.endswith("/a03312019ex101aircraft.htm"))
        self.assertIn("/93410/", url)


if __name__ == "__main__":
    unittest.main()

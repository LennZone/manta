"""Tests for the Hello Python example: the English and German tables match,
and the running plugin answers `locale_changed` with German titles. Run
from the plugin folder with `python3 -m unittest discover -s tests`."""

import json
import os
import re
import subprocess
import sys
import tempfile
import time
import unittest

PLUGIN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, PLUGIN)
import main  # noqa: E402

PLACEHOLDER = re.compile(r"\{([a-z_]+)\}")


class Tables(unittest.TestCase):
    def test_same_keys_and_placeholders(self):
        en, de = main.STRINGS["en"], main.STRINGS["de"]
        self.assertEqual(set(en), set(de))
        for k in en:
            self.assertEqual(set(PLACEHOLDER.findall(en[k])), set(PLACEHOLDER.findall(de[k])), k)

    def test_german_is_translated(self):
        en, de = main.STRINGS["en"], main.STRINGS["de"]
        self.assertEqual([k for k in en if en[k] == de[k] and k not in main.SAME_IN_GERMAN], [])

    def test_every_used_key_exists(self):
        with open(os.path.join(PLUGIN, "main.py"), encoding="utf-8") as f:
            src = f.read()
        used = re.findall(r'\b(_|plural)\("([a-z_.]+)"', src)
        self.assertGreater(len(used), 20)
        for fn, key in used:
            if key == "key":  # the example in a comment
                continue
            keys = [key + ".one", key + ".other"] if fn == "plural" else [key]
            for k in keys:
                self.assertIn(k, main.STRINGS["en"])

    def test_sizes_and_plurals(self):
        try:
            main.set_locale("de-DE")
            self.assertEqual(main.human_size(12_400_000), "12,4 MB")
            self.assertEqual(main.plural("items", 1), "1 Objekt")
            main.set_locale("en")
            self.assertEqual(main.human_size(12_400_000), "12.4 MB")
            self.assertEqual(main.plural("items", 3), "3 items")
        finally:
            main.set_locale("en")


class EndToEnd(unittest.TestCase):
    def test_locale_changed_gives_german_titles(self):
        with tempfile.TemporaryDirectory() as tmp:
            proc = subprocess.Popen(
                [sys.executable, "-B", os.path.join(PLUGIN, "main.py")], cwd=PLUGIN,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
            )

            def send(msg):
                proc.stdin.write(json.dumps(msg) + "\n")
                proc.stdin.flush()

            def read_until(pred):
                end = time.time() + 10
                while time.time() < end:
                    line = proc.stdout.readline()
                    if not line:
                        break
                    msg = json.loads(line)
                    if pred(msg):
                        return msg
                raise AssertionError("message not received")

            def titles(m):
                return {c["id"]: c["title"] for c in m["commands"]}

            try:
                pane = {"pane": "left", "path": tmp, "local_path": tmp, "protocol": "Local",
                        "backend_label": "Local", "selection": [], "active": True}
                send({"type": "init", "api_version": 1, "plugin_id": "hello-python", "data_dir": tmp,
                      "panes": [pane], "locale": "en"})
                msg = read_until(lambda m: m["type"] == "register_commands")
                self.assertEqual(titles(msg)["say-hello"], "Say Hello")
                send({"type": "locale_changed", "locale": "de"})
                msg = read_until(lambda m: m["type"] == "register_commands")
                self.assertEqual(titles(msg), {"file-info": "Dateiinformationen zeigen", "say-hello": "Hallo sagen"})
                chips = read_until(lambda m: m["type"] == "set_chips")
                self.assertEqual(chips["chips"][0]["text"], "0 Objekte")
            finally:
                send({"type": "shutdown"})
                proc.stdin.close()
                proc.wait(timeout=5)
                proc.stdout.close()


if __name__ == "__main__":
    unittest.main()

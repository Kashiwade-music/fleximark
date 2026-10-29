from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import task_watch
from distribution import digest


class WatchResourceTests(unittest.TestCase):
    def test_unchanged_files_are_not_read_again_and_changes_refresh_the_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            product = Path(temporary)
            runtime = product / "runtime"
            runtime.mkdir()
            first, second = runtime / "daemon", runtime / "preview.js"
            first.write_bytes(b"native binary")
            second.write_bytes(b"first script")
            manifest = runtime / "manifest.json"
            manifest.write_text(json.dumps({"files": {p.name: digest(p) for p in (first, second)}}), encoding="utf-8")
            memo = {}
            with patch.object(task_watch, "digest", wraps=digest) as hashing:
                task_watch.refresh_development_resources(product, memo)
                self.assertEqual(hashing.call_count, 2)
                initial_manifest = manifest.read_bytes()
                task_watch.refresh_development_resources(product, memo)
                self.assertEqual(hashing.call_count, 2)
                self.assertEqual(manifest.read_bytes(), initial_manifest)
                previous = second.stat()
                second.write_bytes(b"other script")
                os.utime(second, ns=(previous.st_atime_ns, previous.st_mtime_ns + 1_000_000))
                task_watch.refresh_development_resources(product, memo)
                self.assertEqual(hashing.call_count, 3)
                self.assertEqual(json.loads(manifest.read_text(encoding="utf-8"))["files"][second.name], digest(second))
                value = json.loads(manifest.read_text(encoding="utf-8"))
                del value["files"][first.name]
                manifest.write_text(json.dumps(value), encoding="utf-8")
                task_watch.refresh_development_resources(product, memo)
                self.assertNotIn(first.name, memo)

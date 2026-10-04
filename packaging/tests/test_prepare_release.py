import importlib.util
import hashlib
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("prepare_release", Path(__file__).parents[1] / "prepare_release.py")
prepare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare)
REPO = "owner/repo"
TAG = "v1.2.3"
SHA = "1" * 40


class PrepareReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tag_sha = SHA
        self.releases = []
        self.writes = []
        self.api_patch = patch.object(prepare, "api", side_effect=self.api)
        self.api_patch.start()
        self.addCleanup(self.api_patch.stop)

    def api(self, endpoint, *, payload=None, paginate=False):
        if payload is not None:
            self.writes.append((endpoint, payload))
            self.tag_sha = payload["sha"]
            return {}
        if endpoint.endswith("releases?per_page=100"):
            self.assertTrue(paginate)
            return [self.releases]
        if "/git/matching-refs/" in endpoint:
            return [] if self.tag_sha is None else [{"ref": f"refs/tags/{TAG}", "object": {"type": "commit", "sha": self.tag_sha}}]
        self.fail(f"Unexpected API call: {endpoint}")

    def test_existing_matching_tag_does_not_write(self):
        prepare.guard(REPO, TAG, SHA, create=True)
        self.assertEqual(self.writes, [])

    def test_manual_new_tag_checks_are_read_only(self):
        self.tag_sha = None
        prepare.guard(REPO, TAG, SHA, allow_missing=True)
        self.assertEqual(self.writes, [])

    def test_new_tag_created_at_checked_sha_only_in_prepare(self):
        self.tag_sha = None
        prepare.guard(REPO, TAG, SHA, allow_missing=True, create=True)
        self.assertEqual(self.writes, [(f"repos/{REPO}/git/refs", {"ref": f"refs/tags/{TAG}", "sha": SHA})])

    def test_missing_tag_requires_explicit_manual_path(self):
        self.tag_sha = None
        with self.assertRaisesRegex(ValueError, "missing or differs"):
            prepare.guard(REPO, TAG, SHA, create=True)
        self.assertEqual(self.writes, [])

    def test_existing_mismatched_tag_never_moves(self):
        self.tag_sha = "2" * 40
        with self.assertRaisesRegex(ValueError, "missing or differs"):
            prepare.guard(REPO, TAG, SHA, allow_missing=True, create=True)
        self.assertEqual(self.writes, [])

    def test_public_release_never_mutated(self):
        self.releases = [{"tag_name": TAG, "draft": False}]
        with self.assertRaisesRegex(ValueError, "already published"):
            prepare.guard(REPO, TAG, SHA, allow_missing=True, create=True)
        self.assertEqual(self.writes, [])

    def test_existing_draft_allowed(self):
        self.releases = [{"tag_name": TAG, "draft": True}]
        prepare.guard(REPO, TAG, SHA, create=True)
        self.assertEqual(self.writes, [])

    def test_ambiguous_releases_rejected(self):
        self.releases = [{"tag_name": TAG, "draft": True}] * 2
        with self.assertRaisesRegex(ValueError, "Multiple releases"):
            prepare.guard(REPO, TAG, SHA)

    def test_invalid_source_sha_or_tag_cannot_write(self):
        for tag, sha in ((TAG, "main"), (TAG, "1" * 39), (TAG, "A" * 40), ("v1.2.3-rc1", SHA), ("v1.2.3\n", SHA)):
            with self.subTest(tag=tag, sha=sha), self.assertRaises(ValueError):
                prepare.guard(REPO, tag, sha, allow_missing=True, create=True)
        self.assertEqual(self.writes, [])

    def test_annotated_tag_resolves_to_commit(self):
        with patch.object(prepare, "api", side_effect=[
            [{"ref": f"refs/tags/{TAG}", "object": {"type": "tag", "sha": "2" * 40}}],
            {"object": {"type": "commit", "sha": SHA}},
        ]):
            self.assertEqual(prepare.remote_tag_sha(REPO, TAG), SHA)

    def test_similar_prefix_tag_not_selected(self):
        with patch.object(prepare, "api", return_value=[{"ref": "refs/tags/v1.2.30", "object": {"type": "commit", "sha": SHA}}]):
            self.assertIsNone(prepare.remote_tag_sha(REPO, TAG))

    def test_tag_race_after_create_detected(self):
        with patch.object(prepare, "remote_tag_sha", side_effect=[None, "2" * 40]):
            with self.assertRaisesRegex(ValueError, "changed"):
                prepare.guard(REPO, TAG, SHA, allow_missing=True, create=True)

    def test_published_race_after_create_detected(self):
        with patch.object(prepare, "assert_draft_only", side_effect=[None, ValueError("published")]):
            with self.assertRaisesRegex(ValueError, "published"):
                prepare.guard(REPO, TAG, SHA, create=True)

    def test_uploaded_assets_are_complete_and_match_server_digests(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            names = [f"oxidify-{TAG}-{index}" for index in range(7)] + ["checksums.txt", "provenance.json"]
            assets = []
            for name in names:
                (directory / name).write_bytes(b"payload")
                assets.append({"name": name, "size": 7, "state": "uploaded", "digest": "sha256:" + hashlib.sha256(b"payload").hexdigest()})
            with patch.object(prepare, "remote_tag_sha", return_value=SHA), patch.object(prepare, "assert_draft_only", return_value={"id": 42}), patch.object(prepare, "api", return_value=[assets]):
                prepare.verify_uploaded(REPO, TAG, SHA, directory)
                for field, value in (("digest", "bad"), ("size", 8), ("state", "pending"), ("name", "extra")):
                    original = assets[0][field]
                    assets[0][field] = value
                    with self.subTest(field=field), self.assertRaises(ValueError):
                        prepare.verify_uploaded(REPO, TAG, SHA, directory)
                    assets[0][field] = original
                assets.pop()
                with self.assertRaisesRegex(ValueError, "assets differ"):
                    prepare.verify_uploaded(REPO, TAG, SHA, directory)

    def test_publish_takes_only_a_complete_draft_public(self):
        draft = {"id": 42, "tag_name": TAG, "draft": True}
        assets = [{"name": name, "size": 7, "state": "uploaded"} for name in prepare.expected_asset_names(TAG)]
        writes = []
        state = {"draft": True}

        def api(endpoint, *, payload=None, paginate=False, method="POST"):
            if payload is not None:
                writes.append((method, endpoint, payload))
                state["draft"] = payload["draft"]
                return {}
            if "/git/matching-refs/" in endpoint:
                return [{"ref": f"refs/tags/{TAG}", "object": {"type": "commit", "sha": SHA}}]
            if endpoint.endswith("releases?per_page=100"):
                return [[draft]]
            if endpoint.endswith("/assets?per_page=100"):
                return [assets]
            if endpoint.endswith("/releases/42"):
                return {"id": 42, "tag_name": TAG, "draft": state["draft"]}
            self.fail(f"Unexpected API call: {endpoint}")

        with patch.object(prepare, "api", side_effect=api):
            self.assertEqual(len(assets), 9)
            missing = assets.pop()
            with self.assertRaisesRegex(ValueError, "incomplete"):
                prepare.publish(REPO, TAG, SHA)
            assets.append(missing)
            assets[0]["state"] = "pending"
            with self.assertRaisesRegex(ValueError, "not fully uploaded"):
                prepare.publish(REPO, TAG, SHA)
            assets[0]["state"] = "uploaded"
            with self.assertRaisesRegex(ValueError, "differs"):
                prepare.publish(REPO, TAG, "2" * 40)
            self.assertEqual(writes, [], "a failed check must not touch the release")
            prepare.publish(REPO, TAG, SHA)
            self.assertEqual(writes, [("PATCH", f"repos/{REPO}/releases/42",
                                       {"draft": False, "make_latest": "true", "tag_name": TAG, "target_commitish": SHA})])
            # Once public, the same command refuses rather than touching it again.
            draft["draft"] = False
            with self.assertRaisesRegex(ValueError, "already published"):
                prepare.publish(REPO, TAG, SHA)
            self.assertEqual(len(writes), 1)


if __name__ == "__main__":
    unittest.main()

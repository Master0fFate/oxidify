#!/usr/bin/env python3
"""Guard a draft-only GitHub release and, after validation, create its tag.

The workflow calls `check` before CI and `prepare` only after artifact checks.
Uses the runner's existing gh/GITHUB_TOKEN permissions; never moves a tag or
publishes a release. Public releases must be replaced with a new version.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def require(condition, message):
    if not condition:
        raise ValueError(message)


def api(endpoint, *, payload=None, paginate=False):
    command = ["gh", "api", endpoint]
    if paginate:
        command += ["--paginate", "--slurp"]
    if payload is not None:
        command += ["--method", "POST", "--input", "-"]
    return json.loads(subprocess.check_output(command, input=json.dumps(payload) if payload else None, text=True))


def validate_inputs(repository, tag, sha):
    require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository), "Expected owner/repository")
    require(re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag), "Expected a stable vX.Y.Z tag")
    require(re.fullmatch(r"[0-9a-f]{40}", sha), "Expected a full lowercase 40-character commit SHA")


def assert_draft_only(repository, tag):
    pages = api(f"repos/{repository}/releases?per_page=100", paginate=True)
    matches = [item for page in pages for item in page if item.get("tag_name") == tag]
    require(len(matches) <= 1, "Multiple releases already use this tag")
    require(all(item.get("draft") is True for item in matches),
            "Refusing to modify an already published release; use a new version")
    return matches[0] if matches else None


def remote_tag_sha(repository, tag):
    references = api(f"repos/{repository}/git/matching-refs/tags/{tag}")
    matches = [item for item in references if item.get("ref") == f"refs/tags/{tag}"]
    require(len(matches) <= 1, "Ambiguous tag reference")
    if not matches:
        return None
    obj = matches[0]["object"]
    # Annotated tags can point to other tags. Follow them without moving refs.
    for _ in range(10):
        if obj["type"] == "commit":
            return obj["sha"]
        require(obj["type"] == "tag", "Release tag does not resolve to a commit")
        obj = api(f"repos/{repository}/git/tags/{obj['sha']}")["object"]
    raise ValueError("Release tag nesting exceeds ten annotated tags")


def guard(repository, tag, sha, allow_missing=False, create=False):
    validate_inputs(repository, tag, sha)
    assert_draft_only(repository, tag)
    actual = remote_tag_sha(repository, tag)
    require(actual == sha or (actual is None and allow_missing),
            "Release tag is missing or differs from the validated source SHA")
    if create:
        if actual is None:
            # A racing create fails rather than overwriting anybody else's tag.
            api(f"repos/{repository}/git/refs", payload={"ref": f"refs/tags/{tag}", "sha": sha})
        require(remote_tag_sha(repository, tag) == sha, "Release tag changed before draft creation")
        assert_draft_only(repository, tag)


def verify_uploaded(repository, tag, sha, directory):
    validate_inputs(repository, tag, sha)
    require(remote_tag_sha(repository, tag) == sha, "Uploaded release tag differs from checked source")
    draft = assert_draft_only(repository, tag)
    require(draft is not None, "Expected the uploaded draft release")
    pages = api(f"repos/{repository}/releases/{draft['id']}/assets?per_page=100", paginate=True)
    assets = [asset for page in pages for asset in page]
    files = {path.name: path for path in directory.iterdir()
             if path.name.startswith(f"oxidify-{tag}-") or path.name in ("checksums.txt", "provenance.json")}
    require(len(files) == 9, "Expected seven payloads, provenance, and checksums")
    require(len(assets) == len(files) and {a["name"] for a in assets} == set(files),
            "Uploaded draft assets differ from the complete local set")
    for asset in assets:
        path = files[asset["name"]]
        with path.open("rb") as file:
            digest = "sha256:" + hashlib.file_digest(file, "sha256").hexdigest()
        require(asset.get("state") == "uploaded" and asset.get("size") == path.stat().st_size
                and asset.get("digest") == digest, f"Uploaded draft asset differs: {path.name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "prepare", "uploaded"))
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--tag", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--allow-missing", action="store_true")
    parser.add_argument("--directory", type=Path, default=Path("dist"))
    args = parser.parse_args()
    if args.command == "uploaded":
        verify_uploaded(args.repository, args.tag, args.sha, args.directory)
    else:
        guard(args.repository, args.tag, args.sha, args.allow_missing, args.command == "prepare")


if __name__ == "__main__":
    main()

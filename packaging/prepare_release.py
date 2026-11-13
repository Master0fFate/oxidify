#!/usr/bin/env python3
"""Guard a draft-only GitHub release and, after validation, create its tag.

The release workflow calls `check` before CI and `prepare` only after
artifact checks; it never moves a tag or publishes a release. `publish` is
the one deliberate step that makes a release public, run by the maintainer
through the Publish release workflow once the draft holds the complete
asset set. Public releases must be replaced with a new version, except
through `retract`, which the Retract release workflow runs to delete a
release and its tag together when the maintainer pulls one back.
"""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def api(endpoint, *, payload=None, paginate=False, method="POST"):
    command = ["gh", "api", endpoint]
    if paginate:
        command += ["--paginate", "--slurp"]
    if method == "DELETE":
        command += ["--method", "DELETE"]
        subprocess.check_output(command, text=True)
        return {}
    if payload is not None:
        command += ["--method", method, "--input", "-"]
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


def verify_uploaded(repository, tag, sha, directory, manifest_path=None):
    validate_inputs(repository, tag, sha)
    require(remote_tag_sha(repository, tag) == sha, "Uploaded release tag differs from checked source")
    draft = assert_draft_only(repository, tag)
    require(draft is not None, "Expected the uploaded draft release")
    pages = api(f"repos/{repository}/releases/{draft['id']}/assets?per_page=100", paginate=True)
    assets = [asset for page in pages for asset in page]
    files = {path.name: path for path in directory.iterdir()
             if path.name.startswith(f"oxidify-{tag}-") or path.name in ("checksums.txt", "provenance.json")}
    require(set(files) == expected_asset_names(tag), "Expected seven payloads, provenance, and checksums")
    require(len(assets) == len(files) and {a["name"] for a in assets} == set(files),
            "Uploaded draft assets differ from the complete local set")
    for asset in assets:
        path = files[asset["name"]]
        with path.open("rb") as file:
            digest = "sha256:" + hashlib.file_digest(file, "sha256").hexdigest()
        require(asset.get("state") == "uploaded" and asset.get("size") == path.stat().st_size > 0
                and asset.get("digest") == digest, f"Uploaded draft asset differs: {path.name}")
    if manifest_path is not None:
        run_id = os.environ.get("GITHUB_RUN_ID", "")
        attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
        event = os.environ.get("GITHUB_EVENT_NAME", "")
        head_sha = os.environ.get("GITHUB_SHA", "")
        require(os.environ.get("GITHUB_REPOSITORY") == repository, "Workflow repository differs")
        require(re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[1-9][0-9]*", attempt),
                "Missing workflow run identity")
        require(event in ("push", "workflow_dispatch") and re.fullmatch(r"[0-9a-f]{40}", head_sha),
                "Missing workflow event or head SHA")
        manifest = {
            "schema_version": 1, "repository": repository, "tag": tag, "source_sha": sha,
            "run_id": int(run_id), "run_attempt": int(attempt), "event": event, "head_sha": head_sha,
            "release_id": draft["id"],
            "assets": {asset["name"]: {"size": asset["size"], "digest": asset["digest"]} for asset in assets},
        }
        manifest_path.write_text(json.dumps(manifest, sort_keys=True) + "\n", encoding="utf-8")


def expected_asset_names(tag):
    """Every file a complete draft carries: seven payloads, checksums, provenance."""
    prefix = f"oxidify-{tag}-"
    names = {"checksums.txt", "provenance.json", f"{prefix}macos-universal.dmg"}
    for target in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"):
        names.add(f"{prefix}{target}.tar.gz")
    for target in ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"):
        names.update({f"{prefix}{target}.zip", f"{prefix}{target}-setup.exe"})
    return names


def release_run(repository, run_id, workflow_id):
    run = api(f"repos/{repository}/actions/runs/{run_id}")
    require(run.get("id") == run_id and run.get("workflow_id") == workflow_id
            and run.get("path") == ".github/workflows/release.yml", "Unrelated Release workflow run")
    require(run.get("repository", {}).get("full_name") == repository
            and run.get("head_repository", {}).get("full_name") == repository,
            "Release workflow repository differs")
    require(run.get("status") == "completed" and run.get("conclusion") == "success",
            "Release workflow run is not successful and completed")
    require(run.get("event") in ("push", "workflow_dispatch")
            and type(run.get("run_attempt")) is int and run["run_attempt"] > 0
            and re.fullmatch(r"[0-9a-f]{40}", run.get("head_sha", "")),
            "Invalid Release workflow run identity")
    return run


def trusted_manifest(repository, tag, sha, run_id):
    workflow = api(f"repos/{repository}/actions/workflows/release.yml")
    require(type(workflow.get("id")) is int and workflow.get("path") == ".github/workflows/release.yml",
            "Release workflow identity differs")
    run = release_run(repository, run_id, workflow["id"])
    if run["event"] == "push":
        require(run.get("head_branch") == tag and run["head_sha"] == sha,
                "Release push tag or source differs")
    pages = api(f"repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100", paginate=True)
    name = f"publish-manifest-{run['run_attempt']}"
    matches = [item for page in pages for item in page["artifacts"] if item.get("name") == name]
    require(len(matches) == 1, "Current Release attempt manifest is missing or ambiguous")
    artifact = matches[0]
    provenance = artifact.get("workflow_run", {})
    require(type(run.get("repository", {}).get("id")) is int
            and type(run.get("head_repository", {}).get("id")) is int,
            "Release workflow repository identity is missing")
    require(artifact.get("expired") is False and type(artifact.get("id")) is int
            and type(artifact.get("size_in_bytes")) is int and 0 < artifact["size_in_bytes"] <= 1048576
            and provenance.get("id") == run_id and provenance.get("head_sha") == run["head_sha"]
            and provenance.get("repository_id") == run.get("repository", {}).get("id")
            and provenance.get("head_repository_id") == run.get("head_repository", {}).get("id"),
            "Release manifest artifact provenance is invalid or expired")
    digest = artifact.get("digest")
    require(isinstance(digest, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", digest),
            "Release manifest artifact digest is missing")
    archive = subprocess.check_output(["gh", "api", f"repos/{repository}/actions/artifacts/{artifact['id']}/zip"])
    require(len(archive) <= 1048576 and "sha256:" + hashlib.sha256(archive).hexdigest() == digest,
            "Release manifest artifact digest differs")
    with zipfile.ZipFile(io.BytesIO(archive)) as zipped:
        entries = zipped.infolist()
        require(len(entries) == 1 and entries[0].filename == "publish-manifest.json"
                and 0 < entries[0].file_size <= 65536, "Release manifest archive is invalid")
        manifest = json.loads(zipped.read(entries[0]))
    require(isinstance(manifest, dict) and manifest.get("schema_version") == 1
            and manifest.get("repository") == repository and manifest.get("tag") == tag
            and manifest.get("source_sha") == sha and manifest.get("run_id") == run_id
            and manifest.get("run_attempt") == run["run_attempt"]
            and manifest.get("event") == run["event"] and manifest.get("head_sha") == run["head_sha"]
            and type(manifest.get("release_id")) is int and manifest["release_id"] > 0,
            "Release manifest identity differs")
    assets = manifest.get("assets")
    require(isinstance(assets, dict) and set(assets) == expected_asset_names(tag),
            "Release manifest asset names differ")
    require(all(isinstance(asset, dict) and type(asset.get("size")) is int and asset["size"] > 0
                and isinstance(asset.get("digest"), str)
                and re.fullmatch(r"sha256:[0-9a-f]{64}", asset["digest"]) for asset in assets.values()),
            "Release manifest asset sizes or digests are invalid")
    return run, manifest


def publish(repository, tag, sha, run_id):
    validate_inputs(repository, tag, sha)
    require(isinstance(run_id, str) and re.fullmatch(r"[1-9][0-9]*", run_id), "Expected a Release run ID")
    run_id = int(run_id)
    run, manifest = trusted_manifest(repository, tag, sha, run_id)
    require(remote_tag_sha(repository, tag) == sha, "Release tag differs from the checked source SHA")
    draft = assert_draft_only(repository, tag)
    require(draft is not None and draft.get("id") == manifest.get("release_id"),
            "Draft release differs from the validated release")
    require(release_run(repository, run_id, run["workflow_id"]) == run,
            "Release workflow run changed before publication")
    pages = api(f"repos/{repository}/releases/{draft['id']}/assets?per_page=100", paginate=True)
    assets = [asset for page in pages for asset in page]
    names = {asset["name"] for asset in assets}
    expected = expected_asset_names(tag)
    require(len(assets) == len(expected) and names == expected,
            f"Draft assets are incomplete: missing {sorted(expected - names)}, unexpected {sorted(names - expected)}")
    require(all(asset.get("state") == "uploaded" and asset.get("size") == manifest["assets"][asset["name"]]["size"]
                and asset.get("digest") == manifest["assets"][asset["name"]]["digest"] for asset in assets),
            "A draft asset differs from the validated upload")
    api(f"repos/{repository}/releases/{draft['id']}",
        payload={"draft": False, "make_latest": "true", "tag_name": tag, "target_commitish": sha},
        method="PATCH")
    published = api(f"repos/{repository}/releases/{draft['id']}")
    require(published.get("draft") is False and published.get("tag_name") == tag,
            "The release did not leave draft")


def retract(repository, tag, sha):
    """Take a release down again: delete the release for `tag` and the tag
    itself, once the tag still points at the commit named. This is the one
    command that removes anything, so it insists on the exact pair."""
    validate_inputs(repository, tag, sha)
    require(remote_tag_sha(repository, tag) == sha, "Release tag differs from the SHA given; nothing removed")
    pages = api(f"repos/{repository}/releases?per_page=100", paginate=True)
    matches = [item for page in pages for item in page if item.get("tag_name") == tag]
    require(len(matches) <= 1, "Multiple releases use this tag; nothing removed")
    for release in matches:
        api(f"repos/{repository}/releases/{release['id']}", payload={}, method="DELETE")
    api(f"repos/{repository}/git/refs/tags/{tag}", payload={}, method="DELETE")
    require(remote_tag_sha(repository, tag) is None, "The tag is still there")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "prepare", "uploaded", "publish", "retract"))
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--tag", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--allow-missing", action="store_true")
    parser.add_argument("--directory", type=Path, default=Path("dist"))
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--run-id", default="")
    args = parser.parse_args()
    if args.command == "uploaded":
        verify_uploaded(args.repository, args.tag, args.sha, args.directory, args.manifest)
    elif args.command == "publish":
        publish(args.repository, args.tag, args.sha, args.run_id)
    elif args.command == "retract":
        retract(args.repository, args.tag, args.sha)
    else:
        guard(args.repository, args.tag, args.sha, args.allow_missing, args.command == "prepare")


if __name__ == "__main__":
    main()

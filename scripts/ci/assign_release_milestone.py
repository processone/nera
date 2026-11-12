#!/usr/bin/env python3
"""Give a newly opened pull request the milestone of the release being targeted.

The target is the title held by .github/release-target.json on the default
branch; this script owns how that title becomes a milestone and when it is
applied. See docs/RELEASE.md (Release target). It is a port of Draftline's
scripts/assign-release-milestone.mjs and keeps its rules:

  * It resolves an existing open milestone by exact title and creates nothing.
    A target that is missing, closed, ambiguous or unreadable fails the run
    instead of being replaced by a guess.
  * A pull request that already has a milestone is left as its author set it.
  * It reads the pull request again just before writing, and checks that GitHub
    applied the milestone.

    assign_release_milestone.py --target .github/release-target.json

Environment: GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_EVENT_PATH and, optionally,
GITHUB_API_URL.
"""
import argparse
import json
import os
import sys
import urllib.error
import urllib.request

PER_PAGE = 100


def parse_target(path: str) -> str:
    try:
        with open(path, encoding="utf-8") as handle:
            title = json.load(handle).get("milestone")
    except (OSError, ValueError, AttributeError) as error:
        raise RuntimeError(f"Release target {path} is unreadable: {error}")
    if not isinstance(title, str) or title.strip() == "":
        raise RuntimeError(f"Release target {path} has no milestone title")
    return title.strip()


def github(method: str, route: str, body=None):
    base = os.environ.get("GITHUB_API_URL") or "https://api.github.com"
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(
        base + route,
        data=data,
        method=method,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {os.environ.get('GITHUB_TOKEN')}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(
            f"{method} {route} failed with {error.code}: {error.read().decode(errors='replace')}"
        )


def open_milestones_titled(repository: str, title: str) -> list:
    found = []
    page = 1
    while True:
        batch = github(
            "GET", f"/repos/{repository}/milestones?state=open&per_page={PER_PAGE}&page={page}"
        )
        found.extend(m for m in batch if m.get("title") == title)
        if len(batch) < PER_PAGE:
            return found
        page += 1


def run(target_file: str) -> None:
    title = parse_target(target_file)
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as handle:
        event = json.load(handle)
    if event.get("action") != "opened":
        print(f"Event {event.get('action')} does not open a pull request; nothing to do.")
        return
    repository = os.environ["GITHUB_REPOSITORY"]
    number = event["pull_request"]["number"]

    matches = open_milestones_titled(repository, title)
    if not matches:
        raise RuntimeError(f"Release target milestone {title} does not exist or is closed")
    if len(matches) > 1:
        raise RuntimeError(
            f"Release target milestone {title} is held by {len(matches)} open milestones"
        )

    # The event is a snapshot and the lookup above takes time: read the pull
    # request at the last moment so a milestone chosen meanwhile is kept.
    current = github("GET", f"/repos/{repository}/pulls/{number}")
    if current.get("milestone"):
        print(f"#{number} already has milestone {current['milestone']['title']}; kept.")
        return

    updated = github("PATCH", f"/repos/{repository}/issues/{number}", {"milestone": matches[0]["number"]})
    if (updated.get("milestone") or {}).get("number") != matches[0]["number"]:
        raise RuntimeError(f"GitHub did not apply milestone {title} to #{number}")
    print(f"#{number} assigned to milestone {title}.")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--target", required=True, help="path of release-target.json")
    args = parser.parse_args(argv)
    try:
        run(args.target)
    except RuntimeError as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

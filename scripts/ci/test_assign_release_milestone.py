"""Tests for scripts/ci/assign_release_milestone.py.

Intent: the captain asked that every new pull request join the next release's
milestone, as in Draftline. These replay the 17 cases of Draftline's
tests/assign-release-milestone.test.mjs against a stand-in GitHub REST server on
this computer; nothing reaches GitHub.
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "assign_release_milestone.py")
REPO = "owner/repo"


def milestone(number, title, state="open"):
    """A milestone as GitHub's REST API returns it, reduced to what matters."""
    return {"number": number, "title": title, "state": state, "open_issues": 1, "closed_issues": 0}


def pull(number, ms=None, draft=False):
    return {"number": number, "state": "open", "draft": draft, "milestone": ms}


class FakeGitHub:
    """The routes the script uses, a log of every request, and the data it was given."""

    def __init__(self, milestones=(), pulls=None, patch_status=200, echo="applied", after_listing=None):
        self.milestones = list(milestones)
        self.pulls = dict(pulls or {})
        self.patch_status = patch_status
        self.echo = echo
        self.after_listing = after_listing
        self.requests = []
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def handle_any(self):
                url = urlparse(self.path)
                query = {k: v[0] for k, v in parse_qs(url.query).items()}
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length).decode() if length else ""
                body = json.loads(raw) if raw else None
                fake.requests.append({
                    "method": self.command, "path": url.path, "query": query, "body": body,
                    "authorization": self.headers.get("Authorization"),
                })
                status, value = fake.route(self.command, url.path, query, body)
                data = json.dumps(value).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            do_GET = do_PATCH = do_POST = handle_any

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()

    def route(self, method, path, query, body):
        if method == "GET" and path == f"/repos/{REPO}/milestones":
            per_page = int(query.get("per_page", 30))
            page = int(query.get("page", 1))
            wanted = query.get("state", "open")
            matching = [m for m in self.milestones if wanted == "all" or m["state"] == wanted]
            listing = matching[(page - 1) * per_page: page * per_page]
            # Lets a test change the world while the script is still looking.
            if self.after_listing:
                self.after_listing(self.pulls)
            return 200, listing
        found = re.fullmatch(rf"/repos/{REPO}/pulls/(\d+)", path)
        if method == "GET" and found and int(found[1]) in self.pulls:
            return 200, self.pulls[int(found[1])]
        found = re.fullmatch(rf"/repos/{REPO}/issues/(\d+)", path)
        if method == "PATCH" and found:
            if self.patch_status != 200:
                return self.patch_status, {"message": "Resource not accessible by integration"}
            number = int(found[1])
            chosen = next((m for m in self.milestones if m["number"] == body["milestone"]), None)
            # A write that took effect is visible to the next read, as on GitHub.
            if self.echo == "applied":
                self.pulls[number] = {**self.pulls.get(number, {}), "milestone": chosen}
                return 200, {"number": number, "milestone": chosen}
            if self.echo == "missing":
                return 200, {"number": number, "milestone": None}
            return 200, {"number": number, "milestone": milestone(99, "other")}
        return 404, {"message": "Not Found"}

    def writes(self):
        return [r for r in self.requests if r["method"] != "GET"]


class AssignMilestoneTest(unittest.TestCase):
    def fake(self, **kwargs):
        github = FakeGitHub(**kwargs)
        self.addCleanup(github.close)
        return github

    def run_script(self, github, target='{"milestone":"0.4.0"}\n', number=7, action="opened"):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        target_file = os.path.join(directory.name, "release-target.json")
        event_file = os.path.join(directory.name, "event.json")
        with open(target_file, "w", encoding="utf-8") as handle:
            handle.write(target)
        with open(event_file, "w", encoding="utf-8") as handle:
            json.dump({"action": action, "pull_request": {"number": number}}, handle)
        env = {
            "PATH": os.environ.get("PATH", ""),
            "GITHUB_API_URL": github.url,
            "GITHUB_REPOSITORY": REPO,
            "GITHUB_EVENT_PATH": event_file,
            "GITHUB_TOKEN": "test-token",
        }
        done = subprocess.run([sys.executable, "-I", SCRIPT, "--target", target_file],
                              env=env, capture_output=True, text=True, timeout=60)
        return done.returncode, done.stdout + done.stderr

    def test_a_new_pull_request_without_a_milestone_receives_the_target(self):
        github = self.fake(milestones=[milestone(2, "0.4.0"), milestone(1, "0.3.0", "closed")],
                           pulls={7: pull(7)})
        status, output = self.run_script(github)
        self.assertEqual(status, 0, output)
        self.assertEqual(
            [(r["method"], r["path"], r["body"], r["authorization"]) for r in github.writes()],
            [("PATCH", f"/repos/{REPO}/issues/7", {"milestone": 2}, "Bearer test-token")])

    def test_a_draft_pull_request_is_assigned_from_opening_like_any_other(self):
        github = self.fake(milestones=[milestone(2, "0.4.0")], pulls={9: pull(9, None, True)})
        status, output = self.run_script(github, number=9)
        self.assertEqual(status, 0, output)
        self.assertEqual(len(github.writes()), 1)

    def test_the_target_is_found_by_exact_title_across_pages_not_by_prefix(self):
        lookalikes = [milestone(100 + i, f"0.4.0-rc.{i}") for i in range(100)]
        github = self.fake(milestones=lookalikes + [milestone(301, "0.4.01"), milestone(2, "0.4.0")],
                           pulls={7: pull(7)})
        status, output = self.run_script(github)
        self.assertEqual(status, 0, output)
        self.assertEqual(github.writes()[0]["body"], {"milestone": 2})

    def test_an_explicit_milestone_is_kept_and_nothing_is_written(self):
        github = self.fake(milestones=[milestone(2, "0.4.0"), milestone(3, "1.0.0")],
                           pulls={7: pull(7, milestone(3, "1.0.0"))})
        status, output = self.run_script(github)
        self.assertEqual(status, 0, output)
        self.assertEqual(github.writes(), [])

    def test_a_repeated_opened_event_leaves_the_first_assignment_alone(self):
        github = self.fake(milestones=[milestone(2, "0.4.0")], pulls={7: pull(7)})
        self.assertEqual(self.run_script(github)[0], 0)
        self.assertEqual(self.run_script(github)[0], 0)
        self.assertEqual(len(github.writes()), 1)

    def test_switching_the_target_affects_only_pull_requests_opened_afterwards(self):
        github = self.fake(milestones=[milestone(2, "0.4.0"), milestone(4, "0.5.0")],
                           pulls={7: pull(7), 8: pull(8)})
        self.assertEqual(self.run_script(github, number=7)[0], 0)
        status, output = self.run_script(github, number=8, target='{"milestone":"0.5.0"}')
        self.assertEqual(status, 0, output)
        # A repeated opening of the first pull request, now under the new target.
        self.assertEqual(self.run_script(github, number=7, target='{"milestone":"0.5.0"}')[0], 0)
        self.assertEqual([(r["path"], r["body"]["milestone"]) for r in github.writes()],
                         [(f"/repos/{REPO}/issues/7", 2), (f"/repos/{REPO}/issues/8", 4)])

    def test_a_milestone_chosen_while_the_script_was_looking_is_not_overwritten(self):
        def choose(pulls):
            pulls[7] = pull(7, milestone(3, "1.0.0"))
        github = self.fake(milestones=[milestone(2, "0.4.0"), milestone(3, "1.0.0")],
                           pulls={7: pull(7)}, after_listing=choose)
        status, output = self.run_script(github)
        self.assertEqual(status, 0, output)
        self.assertEqual(github.writes(), [])

    def test_an_event_other_than_opened_writes_nothing(self):
        github = self.fake(milestones=[milestone(2, "0.4.0")], pulls={7: pull(7)})
        status, _ = self.run_script(github, action="synchronize")
        self.assertEqual(status, 0)
        self.assertEqual(github.requests, [])

    def test_failures_are_explicit_and_create_no_milestone(self):
        cases = [
            ("a malformed target file", {"target": '{"milestone":'}, {}),
            ("a target without a milestone", {"target": "{}"}, {}),
            ("a blank target", {"target": '{"milestone":"  "}'}, {}),
            ("a target whose milestone does not exist", {},
             {"milestones": [milestone(3, "1.0.0")], "pulls": {7: pull(7)}}),
            ("a target whose milestone is closed", {},
             {"milestones": [milestone(2, "0.4.0", "closed")], "pulls": {7: pull(7)}}),
            ("a target held by two open milestones", {},
             {"milestones": [milestone(2, "0.4.0"), milestone(5, "0.4.0")], "pulls": {7: pull(7)}}),
            ("a write that returns no milestone", {},
             {"milestones": [milestone(2, "0.4.0")], "pulls": {7: pull(7)}, "echo": "missing"}),
            ("a write that returns another milestone", {},
             {"milestones": [milestone(2, "0.4.0")], "pulls": {7: pull(7)}, "echo": "other"}),
            ("a refused write", {},
             {"milestones": [milestone(2, "0.4.0")], "pulls": {7: pull(7)}, "patch_status": 403}),
        ]
        for name, options, world in cases:
            with self.subTest(name):
                github = self.fake(**world)
                status, output = self.run_script(github, **options)
                self.assertNotEqual(status, 0)
                self.assertRegex(output, r"(?i)release|milestone|403")
                self.assertEqual([r for r in github.requests if r["method"] == "POST"], [],
                                 "no milestone is ever created")
                self.assertEqual([r for r in github.requests
                                  if r["method"] == "PATCH" and r["body"]["milestone"] != 2], [])


class ReleaseTargetFileTest(unittest.TestCase):
    """Intent: the target file is the one place the next version is written."""

    def test_the_repository_target_names_one_bare_version(self):
        root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        with open(os.path.join(root, ".github", "release-target.json"), encoding="utf-8") as handle:
            title = json.load(handle)["milestone"]
        self.assertRegex(title, r"^\d+\.\d+\.\d+$")


if __name__ == "__main__":
    unittest.main()

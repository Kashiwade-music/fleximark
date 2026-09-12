from __future__ import annotations

import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
RELEASE_CONFIG = (ROOT / "release.config.mjs").read_text(encoding="utf-8")
PACKAGE_JSON = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))


def job(name: str) -> str:
    marker = f"  {name}:"
    start = WORKFLOW.index(marker)
    remainder = WORKFLOW[start + len(marker) :]
    next_job = re.search(r"^  [a-z0-9-]+:\s*$", remainder, re.MULTILINE)
    if next_job is None:
        return WORKFLOW[start:]
    return WORKFLOW[start : start + len(marker) + next_job.start()]


def run_blocks(source: str) -> list[str]:
    lines = source.splitlines()
    blocks: list[str] = []
    for index, line in enumerate(lines):
        if not line.startswith("        run:"):
            continue
        block = [line]
        for following in lines[index + 1 :]:
            if following and len(following) - len(following.lstrip()) <= 8:
                break
            block.append(following)
        blocks.append("\n".join(block))
    return blocks


class ReleaseWorkflowCharacterizationTests(unittest.TestCase):
    def test_final_release_artifact_is_verified_and_smoked_after_release(self) -> None:
        validate = job("validate")
        clean_install = job("clean-install")
        release = job("release")

        self.assertNotIn("vsce package", validate)
        self.assertNotIn("fleximark-candidate-vsix", WORKFLOW)

        self.assertIn("needs: release", clean_install)
        self.assertIn("if: needs.release.outputs.released == 'true'", clean_install)
        self.assertIn("name: fleximark-release-vsix", clean_install)
        verify = clean_install.index("release_artifact.py verify")
        smoke = clean_install.index("mise run smoke -- fleximark.vsix")
        self.assertLess(verify, smoke)
        self.assertIn(
            "FLEXIMARK_EXPECTED_SHA256: ${{ needs.release.outputs.vsix_sha256 }}",
            clean_install,
        )
        self.assertIn("mise run smoke -- fleximark.vsix", clean_install)
        self.assertIn("needs: [daemon-platforms, validate]", release)

    def test_semantic_release_preserves_version_changelog_and_release_commit(
        self,
    ) -> None:
        ordered_plugins = [
            '"@semantic-release/commit-analyzer"',
            '"@semantic-release/release-notes-generator"',
            '"@semantic-release/changelog"',
            '"@semantic-release/npm"',
            '"./scripts/semantic-release-package.mjs"',
            '"@semantic-release/git"',
            '"@semantic-release/github"',
        ]
        positions = [RELEASE_CONFIG.index(plugin) for plugin in ordered_plugins]
        self.assertEqual(positions, sorted(positions))
        self.assertIn("npmPublish: false", RELEASE_CONFIG)
        self.assertIn("draftRelease: true", RELEASE_CONFIG)
        self.assertIn('path: "fleximark.vsix.identity.json"', RELEASE_CONFIG)
        self.assertIn(
            'assets: ["CHANGELOG.md", "package.json", "yarn.lock"]',
            RELEASE_CONFIG,
        )
        self.assertIn(
            "chore(release): ${nextRelease.version} [skip ci]", RELEASE_CONFIG
        )

    def test_release_invokes_semantic_package_prepare(self) -> None:
        validate = job("validate")
        release = job("release")

        self.assertNotIn("vsce package", validate)
        self.assertEqual(
            PACKAGE_JSON["scripts"]["vscode:prepublish"],
            "python scripts/tasks.py vscode-prepublish",
        )
        self.assertIn(
            "run: yarn exec semantic-release",
            release,
        )

    def test_release_hands_off_vsix_and_identity_with_an_independent_hash_output(
        self,
    ) -> None:
        release = job("release")
        self.assertIn('path: "fleximark.vsix"', RELEASE_CONFIG)
        self.assertIn(
            "vsix_sha256: ${{ steps.release-state.outputs.vsix_sha256 }}", release
        )
        self.assertIn(
            "release_git_tag: ${{ steps.release-state.outputs.release_git_tag }}",
            release,
        )
        self.assertIn(
            "release_git_head: ${{ steps.release-state.outputs.release_git_head }}",
            release,
        )
        self.assertIn(
            "release_source_git_head: ${{ steps.release-state.outputs.release_source_git_head }}",
            release,
        )
        handoff = release.index("name: fleximark-release-vsix")
        self.assertIn("fleximark.vsix.identity.json", release[handoff:])
        self.assertIn("fleximark.vsix", release[handoff:])
        self.assertIn("if-no-files-found: error", release[handoff:])

    def test_release_rerun_recovers_a_complete_source_bound_github_release(
        self,
    ) -> None:
        release = job("release")
        recovery = release.index("Recover prior semantic release")
        semantic = release.index("yarn exec semantic-release")
        state = release.index("id: release-state")
        self.assertLess(recovery, semantic)
        self.assertIn(
            "steps.recovery.outputs.recovered != 'true'", release[recovery:semantic]
        )
        self.assertIn(
            'export FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD="$SOURCE_GIT_HEAD"',
            release[recovery:semantic],
        )
        self.assertIn("git rev-parse", release[recovery:semantic])
        self.assertIn("release_artifact.py verify", release[recovery:semantic])
        self.assertIn("release tag is not unique", release[recovery:semantic])
        self.assertIn("unexpected GitHub Release assets", release[recovery:semantic])
        self.assertIn("steps.recovery.outputs.recovered", release[state:])
        self.assertIn("overwrite: true", release[state:])

    def test_no_release_path_skips_all_publish_side_effects(self) -> None:
        release = job("release")
        clean_install = job("clean-install")
        attestation = job("attest")
        github_release = job("publish-github-release")
        marketplace = job("publish-marketplace")

        self.assertIn('echo "released=false" >> "$GITHUB_OUTPUT"', release)
        self.assertIn("release identity unexpectedly exists", release)
        self.assertIn("released: ${{ steps.release-state.outputs.released }}", release)
        upload = release.index("- name: Upload exact release artifact")
        self.assertIn(
            "if: steps.release-state.outputs.released == 'true'", release[upload:]
        )
        for downstream in (
            clean_install,
            attestation,
            github_release,
            marketplace,
        ):
            self.assertIn("if: needs.release.outputs.released == 'true'", downstream)
        self.assertIn(
            "needs: [release, clean-install, attest, publish-github-release]",
            marketplace,
        )

    def test_attestation_and_marketplace_verify_the_exact_smoked_hash(self) -> None:
        attestation = job("attest")
        github_release = job("publish-github-release")
        marketplace = job("publish-marketplace")

        self.assertIn("needs: [release, clean-install]", attestation)
        self.assertIn("release_artifact.py verify", attestation)
        self.assertIn("FLEXIMARK_EXPECTED_SHA256", attestation)
        self.assertLess(
            attestation.index("release_artifact.py verify"),
            attestation.index("subject-path: fleximark.vsix"),
        )
        self.assertNotIn("draft=false", WORKFLOW[: WORKFLOW.index("  attest:")])
        self.assertIn("needs: [release, clean-install, attest]", github_release)
        verify_draft = github_release.index("Verify exact GitHub Release assets")
        publish_draft = github_release.index("draft=false")
        remote_download = github_release.index("gh release download")
        remote_verify = github_release.index(
            "release_artifact.py verify", remote_download
        )
        self.assertLess(verify_draft, publish_draft)
        self.assertLess(publish_draft, remote_download)
        self.assertLess(remote_download, remote_verify)
        self.assertIn('if [[ "$RELEASE_IS_DRAFT" == "true" ]]', github_release)
        self.assertIn('elif [[ "$RELEASE_IS_DRAFT" != "false" ]]', github_release)
        self.assertIn("fleximark.vsix.identity.json", github_release)
        download = marketplace.index("name: fleximark-release-vsix")
        verify = marketplace.index("release_artifact.py verify")
        publish = marketplace.index("yarn exec vsce publish")
        self.assertIn("--packagePath fleximark.vsix", marketplace[publish:])
        self.assertIn("--oidc", marketplace[publish:])
        self.assertLess(download, verify)
        self.assertLess(verify, publish)
        self.assertNotIn("EXPECTED_VSIX_SHA256", WORKFLOW)
        self.assertIn("publish-github-release", marketplace)
        self.assertIn("--skip-duplicate", marketplace[publish:])
        self.assertNotIn("vsce package", marketplace)

    def test_every_consumer_passes_scoped_identity_expectations(self) -> None:
        for job_name in (
            "clean-install",
            "attest",
            "publish-github-release",
            "publish-marketplace",
        ):
            source = job(job_name)
            with self.subTest(job=job_name):
                for environment_name, output_name in (
                    ("FLEXIMARK_EXPECTED_SHA256", "vsix_sha256"),
                    ("FLEXIMARK_EXPECTED_GIT_TAG", "release_git_tag"),
                    (
                        "FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD",
                        "release_source_git_head",
                    ),
                ):
                    self.assertIn(
                        f"{environment_name}: ${{{{ needs.release.outputs.{output_name} }}}}",
                        source,
                    )
                self.assertIn(
                    "run: python scripts/release_artifact.py verify --vsix fleximark.vsix --identity fleximark.vsix.identity.json",
                    source,
                )
                self.assertNotIn("--expected-git-head", source)

    def test_dynamic_outputs_are_only_interpolated_through_step_environments(
        self,
    ) -> None:
        forbidden = re.compile(
            r"\$\{\{\s*(?:needs\.|steps\.(?:recovery|release-assets)\.)"
        )
        for run in run_blocks(WORKFLOW):
            with self.subTest(run=run.splitlines()[0]):
                self.assertNotRegex(run, forbidden)

    def test_all_upload_artifacts_are_explicitly_overwritten(self) -> None:
        upload_blocks = WORKFLOW.split("uses: actions/upload-artifact@")[1:]
        self.assertGreaterEqual(len(upload_blocks), 2)
        for block in upload_blocks:
            with self.subTest(block=block[:80]):
                self.assertIn("overwrite: true", block.split("\n\n", 1)[0])


if __name__ == "__main__":
    unittest.main()

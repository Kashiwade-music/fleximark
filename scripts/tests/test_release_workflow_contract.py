from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
CI_WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
VERIFIER = (ROOT / ".github/actions/verified-release-artifact/action.yml").read_text(
    encoding="utf-8"
)
RELEASE_CONFIG = (ROOT / "release.config.mjs").read_text(encoding="utf-8")


def job(name: str) -> str:
    marker = f"  {name}:"
    start = WORKFLOW.index(marker)
    following = WORKFLOW[start + len(marker) :]
    match = re.search(r"^  [a-z0-9-]+:\s*$", following, re.MULTILINE)
    return (
        WORKFLOW[start:]
        if match is None
        else WORKFLOW[start : start + len(marker) + match.start()]
    )


def assert_ordered(test: unittest.TestCase, source: str, *markers: str) -> None:
    positions = [source.index(marker) for marker in markers]
    test.assertEqual(positions, sorted(positions))


class ReleaseWorkflowContractTests(unittest.TestCase):
    def test_daemons_are_manifested_before_validation_or_release(self) -> None:
        ci_validate = CI_WORKFLOW[
            CI_WORKFLOW.index("  validate:") : CI_WORKFLOW.index(
                "  dependency-review:"
            )
        ]
        for source in (ci_validate, job("validate")):
            assert_ordered(
                self,
                source,
                "Download platform daemons",
                "create_release_manifest.py --require-all",
                "mise run test -- --prebuilt",
            )
        assert_ordered(
            self,
            job("release"),
            "Download platform daemons",
            "create_release_manifest.py --require-all",
            "yarn exec semantic-release",
        )

    def test_semantic_release_keeps_the_release_commit_and_draft_assets(self) -> None:
        assert_ordered(
            self,
            RELEASE_CONFIG,
            '"@semantic-release/commit-analyzer"',
            '"@semantic-release/release-notes-generator"',
            '"@semantic-release/changelog"',
            '"@semantic-release/npm"',
            '"./scripts/semantic-release-package.mjs"',
            '"@semantic-release/git"',
            '"@semantic-release/github"',
        )
        for contract in (
            "npmPublish: false",
            "draftRelease: true",
            'path: "fleximark.vsix"',
            'path: "fleximark.vsix.identity.json"',
            'assets: ["CHANGELOG.md", "package.json", "yarn.lock"]',
        ):
            self.assertIn(contract, RELEASE_CONFIG)

    def test_release_is_recoverable_and_emits_an_exact_artifact_identity(self) -> None:
        release = job("release")
        assert_ordered(
            self,
            release,
            "Recover prior semantic release",
            "yarn exec semantic-release",
            "id: release-state",
            "name: fleximark-release-vsix",
        )
        for contract in (
            "needs: [daemon-platforms, validate]",
            "steps.recovery.outputs.recovered != 'true'",
            'export FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD="$SOURCE_GIT_HEAD"',
            "release_artifact.py verify",
            "release tag is not unique",
            "release identity unexpectedly exists",
            "if: steps.release-state.outputs.released == 'true'",
            "overwrite: true",
        ):
            self.assertIn(contract, release)
        for output in (
            "release_git_tag",
            "release_git_head",
            "release_source_git_head",
            "vsix_sha256",
        ):
            self.assertIn(
                f"{output}: ${{{{ steps.release-state.outputs.{output} }}}}", release
            )

    def test_consumers_share_identity_verification_and_publish_in_order(self) -> None:
        for contract in (
            "jdx/mise-action@7e36c90d9ab29c415a2384db3006f3ec8a8cc654",
            "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
            "name: fleximark-release-vsix",
            "FLEXIMARK_EXPECTED_SHA256: ${{ inputs.sha256 }}",
            "FLEXIMARK_EXPECTED_GIT_TAG: ${{ inputs.git-tag }}",
            "FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD: ${{ inputs.source-git-head }}",
            "python scripts/release_artifact.py verify",
        ):
            self.assertIn(contract, VERIFIER)

        for name in (
            "clean-install",
            "attest",
            "publish-github-release",
            "publish-marketplace",
        ):
            source = job(name)
            with self.subTest(job=name):
                self.assertIn("if: needs.release.outputs.released == 'true'", source)
                self.assertIn(
                    "uses: ./.github/actions/verified-release-artifact", source
                )
                for input_name, output_name in (
                    ("sha256", "vsix_sha256"),
                    ("git-tag", "release_git_tag"),
                    ("source-git-head", "release_source_git_head"),
                ):
                    self.assertIn(
                        f"{input_name}: ${{{{ needs.release.outputs.{output_name} }}}}",
                        source,
                    )

        clean_install = job("clean-install")
        self.assertLess(
            clean_install.index("verified-release-artifact"),
            clean_install.index("mise run smoke"),
        )
        attestation = job("attest")
        self.assertLess(
            attestation.index("verified-release-artifact"),
            attestation.index("subject-path: fleximark.vsix"),
        )
        github_release = job("publish-github-release")
        assert_ordered(
            self,
            github_release,
            "verified-release-artifact",
            "Verify exact GitHub Release assets",
            "release_artifact.py verify",
            "draft=false",
            "gh release download",
        )
        self.assertGreater(
            github_release.rindex("release_artifact.py verify"),
            github_release.index("gh release download"),
        )
        marketplace = job("publish-marketplace")
        self.assertLess(
            marketplace.index("verified-release-artifact"),
            marketplace.index("yarn exec vsce publish"),
        )
        self.assertIn("--oidc", marketplace)
        self.assertIn("--skip-duplicate", marketplace)

    def test_dynamic_outputs_stay_out_of_shell_and_uploads_overwrite(self) -> None:
        run_blocks = re.findall(
            r"^        run:.*?(?=^      - |\Z)",
            WORKFLOW,
            re.MULTILINE | re.DOTALL,
        )
        forbidden = re.compile(
            r"\$\{\{\s*(?:needs\.|steps\.(?:recovery|release-assets)\.)"
        )
        for block in run_blocks:
            with self.subTest(run=block.splitlines()[0]):
                self.assertNotRegex(block, forbidden)
        uploads = WORKFLOW.split("uses: actions/upload-artifact@")[1:]
        self.assertGreaterEqual(len(uploads), 2)
        self.assertTrue(
            all("overwrite: true" in block.split("\n\n", 1)[0] for block in uploads)
        )


if __name__ == "__main__":
    unittest.main()

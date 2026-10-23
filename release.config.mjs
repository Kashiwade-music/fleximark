export default {
  branches: ["main"],
  plugins: [
    "@semantic-release/commit-analyzer",
    "@semantic-release/release-notes-generator",
    "@semantic-release/changelog",
    "./tooling/tasks/semantic-release-package.mjs",
    [
      "@semantic-release/git",
      {
        assets: [
          "CHANGELOG.md",
          "release.toml",
          "adapters/vscode/package.json",
          "adapters/zed/extension.toml",
          "apps/server/Cargo.toml",
          "apps/cli/Cargo.toml",
          "Cargo.lock",
        ],
        message:
          "chore(release): ${nextRelease.version} [skip ci]\n\n${nextRelease.notes}",
      },
    ],
    [
      "@semantic-release/github",
      {
        successCommentCondition: false,
        failCommentCondition: false,
        labels: false,
        releasedLabels: false,
        draftRelease: true,
      },
    ],
  ],
};

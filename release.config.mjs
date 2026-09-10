export default {
  branches: ["main"],
  plugins: [
    "@semantic-release/commit-analyzer",
    "@semantic-release/release-notes-generator",
    "@semantic-release/changelog",
    [
      "@semantic-release/npm",
      {
        npmPublish: false,
      },
    ],
    "./scripts/semantic-release-package.mjs",
    [
      "@semantic-release/github",
      {
        successCommentCondition: false,
        failCommentCondition: false,
        labels: false,
        releasedLabels: false,
        assets: [
          {
            path: "fleximark.vsix",
            label: "FlexiMark VS Code extension",
          },
        ],
      },
    ],
    [
      "@semantic-release/git",
      {
        assets: ["CHANGELOG.md", "package.json", "package-lock.json"],
        message:
          "chore(release): ${nextRelease.version} [skip ci]\n\n${nextRelease.notes}",
      },
    ],
  ],
};

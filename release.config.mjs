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
      "@semantic-release/git",
      {
        assets: ["CHANGELOG.md", "package.json", "yarn.lock"],
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
        assets: [
          {
            path: "fleximark.vsix",
            label: "FlexiMark VS Code extension",
          },
          {
            path: "fleximark.vsix.identity.json",
            label: "FlexiMark release identity",
          },
        ],
      },
    ],
  ],
};

# Skills

Project-specific skills for openshell-image-builder.

## Available skills

| Skill | Description |
|---|---|
| [`check`](check/SKILL.md) | Run the pre-commit check suite (fmt + clippy + unit tests) and auto-fix formatting if needed |
| [`integration-tests`](integration-tests/SKILL.md) | Run the integration tests in `tests/integration_test.rs` against real container images built with podman |
| [`debug-image`](debug-image/SKILL.md) | Inspect a built test image interactively to diagnose a failing integration test — binaries, policy, config files, ownership |
| [`sandbox-policy`](sandbox-policy/SKILL.md) | Understand and edit the sandbox policy — base YAML, network rule schema, inference and agent fragment merging, testing |
| [`vm-runtime`](vm-runtime/SKILL.md) | Build, sign, and run the `--runtime vm` backend — the libkrun microVM, its embedded root filesystem, and the errors each step can produce |
| [`update-github-action`](update-github-action/SKILL.md) | Add or update a GitHub Actions step — fetch the latest release SHA and write the correctly pinned uses line |
| [`work-on-issue`](work-on-issue/SKILL.md) | Fetch an issue from the upstream repository, map it to existing skills, and produce an implementation plan |
| [`coderabbit-review`](coderabbit-review/SKILL.md) | Fetch all CodeRabbitAI review comments on the current PR, triage by severity, and address the actionable ones |
| [`add-base-image`](add-base-image/SKILL.md) | Step-by-step checklist for adding a new base image (Containerfile, unit tests, integration tests, README) |
| [`add-cli-flag`](add-cli-flag/SKILL.md) | Step-by-step checklist for adding a new CLI flag (clap struct, run(), modules, unit tests, integration tests, README) |
| [`add-skill`](add-skill/SKILL.md) | Create a new project skill in `.agents/skills/` and register it in the skills README |
| [`add-inference`](add-inference/SKILL.md) | Step-by-step checklist for adding a new inference provider (trait impl, agent wiring, unit tests, integration tests, README) |
| [`add-agent`](add-agent/SKILL.md) | Step-by-step checklist for adding a new agent (Agent trait, mod.rs registration, image_tests! macro extension, integration tests, README) |

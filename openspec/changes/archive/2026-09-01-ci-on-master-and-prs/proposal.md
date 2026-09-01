## Why

`./scripts/check.sh` is the gate for every commit — `CLAUDE.md` says so, and it must pass in full
before anything lands. Nothing enforces that. It runs when a developer remembers to run it, on
whatever compiler that developer happens to have, and on one machine. There is no CI at all:
`.github/` holds prompts and skills and no workflows.

Three consequences, and they are not hypothetical for this repository in particular:

- **A guard that is skipped is a guard that does not exist.** Three of the checks are shell
  scripts encoding failure modes that are silent at runtime — `HashMap` iteration order breaking
  seed reproducibility, `io::Error` flattening distinct failures, a socket appearing before
  constraint 5 discharges it. They are mechanical checks precisely because review misses them, and
  they only run if something runs them.
- **A pull request has nothing to say about itself.** There is no signal on a branch other than a
  developer's word that they ran the script.
- **The whole project rests on determinism, and it has only ever been observed on one machine.**
  `openspec/specs/simulation/spec.md` requires that a run is fully determined by its seed and
  configuration. Every measurement in `docs/bounded-space.md` and every seeded property test assumes
  it. That assumption has been checked on exactly one platform, by one person.

## What Changes

- **A GitHub Actions workflow** that runs on pushes to `master` and on pull requests targeting it,
  and whose single job runs `./scripts/check.sh` — the same script, not a reimplementation of it.
- **The build is cached** on the cargo registry and the `target` directory, keyed on `Cargo.lock`.
- **The README's guard table says what enforces it**, since the table currently describes checks
  that run only by hand.
- **Two badges at the top of the README.** One reports the gate — and since it is the workflow's own
  badge rather than a test-only one, red means *any* check is failing, which is what "do not commit
  until these are clean" already treats them as. The other links to the published documentation, so
  the way in is the first thing on the page rather than something to go looking for.
- **The API documentation is built and published to GitHub Pages** from `master` only, and only
  after the gate has passed. Much of this repository's value is in its module documentation — the
  quoted pseudocode, the departures, the space bounds — and it is currently readable only by
  cloning.
- **Broken documentation links become a guard.** `cargo doc` does not currently build clean, and
  what it is complaining about is not cosmetic:

  | | |
  |---|---|
  | `Cmd::Start` in `flooding_consensus` and `uniform_reliable_broadcast` | a command that **no longer exists**, still documented as the way to begin |
  | `crate::link::ScopedLink` in `perfect_link` | deleted in `link-parameterisation`; `link.rs` records at length that it was, and this module still says it deliberately does not implement it |
  | `Link::delivered` in `link.rs` | renamed to `classify` |
  | `Link` ×2, `recon_sim::Sim::delivery_bound`, a public link to a private field | paths that do not resolve |

  Four of the eight are stale prose asserting things that are not there — the failure `CLAUDE.md`
  calls out by name, since "this repository's method is reading code against its quoted contract".
  They are fixed here, and `cargo doc` with warnings denied joins `check.sh` so that the next one
  cannot land. It goes in the script rather than the workflow for the reason the design gives: a
  gate belongs where every gate is, and CI then picks it up without a second edit.

Publishing needs one thing this change cannot do for itself: **Settings → Pages → Source must be set
to "GitHub Actions"**, once, by hand. Until it is, the deploy job fails and the gate is unaffected.

Two decisions taken deliberately, both narrowing scope:

- **Ubuntu only.** A macOS runner would be the first evidence that determinism survives a change of
  platform and architecture — dev is macOS on arm64 — but that doubles the cost of every run, and
  the claim is recorded as untested rather than tested. Noted in `design.md` as the obvious
  extension.
- **No toolchain pin.** CI uses current stable and so does a developer's machine, which means a new
  clippy release can turn a green commit red with no change to the repository. `-D warnings` is a
  hard gate, so that will happen eventually; the trade is that the code always builds on current
  stable and no file constrains local development.

## Capabilities

None. `skip_specs: true` is set in this change's `.openspec.yaml`, which is the honest marker
rather than a convenience: a workflow that runs an existing script changes no behaviour of the
system this repository specifies. The capabilities here are `protocol-core`, `simulation`, `links`,
`broadcast`, `consensus` and `failure-detection` — what the protocols must do — and CI is the
machinery that runs their checks rather than a claim about what they check. Inventing a capability
to satisfy the schema would be writing a specification for a build script.

## Impact

One new file, `.github/workflows/ci.yml`. `scripts/check.sh` gains a check, and eight docstrings
across `recon-protocols` are corrected. `README.md` is dated in four places: its guard table, its
build instructions, its head, which gains the badge, and wherever it can now point at published
documentation. No crate's behaviour changes and no dependency moves. No crate changes, no dependency changes, and nothing that alters a local build.

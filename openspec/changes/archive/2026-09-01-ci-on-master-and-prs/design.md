## Context

See `proposal.md` — Why. The constraint that shapes everything here is that `./scripts/check.sh`
already exists and is already the definition of "clean": seven checks, aggregating rather than
short-circuiting, exit 1 if any failed. CI's job is to run it, not to have an opinion about it.

## Goals / Non-Goals

Goals: the gate runs on `master` and on pull requests targeting it; a failure is legible without
opening a log; the state of `master` is visible from the README; the API documentation is published
and cannot rot unnoticed. Non-goals: a macOS runner, a pinned toolchain, release automation,
coverage, benchmarks, and validating OpenSpec artifacts — each addressed below or in the open
questions.

## Decisions

### CI runs `check.sh`, it does not reimplement it

One step, `./scripts/check.sh`, rather than seven steps mirroring what the script does.

The argument is drift. `CLAUDE.md` names that script as the gate for every commit and the README's
guard table describes its contents; a workflow that lists `cargo fmt`, `cargo clippy`, … separately
is a second definition of "clean" that can disagree with the first, silently, in the direction that
matters — CI passing something a developer's run would have caught, or the reverse. Running the
script means the two are the same thing by construction, and adding a guard to `check.sh` puts it in
CI with no second edit.

What it costs is granularity in the Actions UI: one red step rather than a red `clippy` step. That
is smaller than it looks, because the script prints `PASS:`/`FAIL:` per section and — since it uses
`set -uo pipefail` without `-e` — **runs every check even after one fails**. A single CI run
therefore reports every problem at once, which a step-per-check workflow would not: that stops at
the first failure.

*Alternative — steps per check, sharing a composite action.* Removes the drift but adds a layer to
keep in step with the script, which is the same problem one level down.

### Nothing is installed that the runner does not already have

`ubuntu-latest` ships rustup with a current stable toolchain including `clippy` and `rustfmt`, so the
job needs `rustup update stable` and nothing else. Third-party actions are then limited to caching.

That matters here for the ordinary reason — every action in a workflow is code from someone else
running with a token — and it is worth taking cheaply where it is cheap. `actions/checkout` is
unavoidable. A toolchain action is not.

### Caching, and what it is allowed to affect

`Swatinem/rust-cache` over the cargo registry and `target/`, keyed on the lockfile and the compiler
version. Correctness may not depend on it: a cold cache produces the same result more slowly, and a
poisoned or stale one is a *rebuild*, never a different verdict — `cargo` decides that, not the
cache key.

Worth watching rather than assuming: this workspace builds `--all-targets` across thirty-odd test
binaries, so `target/` is large and may approach the per-repository cache limit. If it does, the
answer is to cache the registry only, which keeps most of the win.

*Alternative — `actions/cache` by hand.* One fewer third-party action, and a cache key for a Rust
build that is right in all the cases `rust-cache` already handles is more subtle than it looks.

### Documenting is a second job, and it publishes nothing from a red build

`needs: check`, `if: github.ref == 'refs/heads/master'`. A pull request builds no documentation and
deploys none; `master` deploys only what a passing gate produced. The alternative — deploying in
parallel with the checks — publishes documentation for a commit that does not compile, which is
worse than publishing nothing.

`cargo doc --workspace --no-deps`: `--no-deps` because the dependencies' documentation is already on
docs.rs and building it is most of the time; `--workspace` because all three crates are the subject.
Public items only — `--document-private-items` would expose a great deal of internal reasoning that
the module documentation already summarises deliberately, and the summary is the better artifact.

`cargo doc` writes no landing page, so the job adds a small hand-written `index.html` listing the
three crates. Redirecting to one of them would be picking a favourite, and the crates' relationship —
core, then the simulator, then the protocols — is itself worth stating on the way in.

### The Pages deployment, and what it needs that this change cannot provide

`actions/upload-pages-artifact` and `actions/deploy-pages`, with `pages: write` and `id-token: write`
scoped to the deploy job alone rather than the workflow. That keeps the gate job on `contents: read`,
which is the whole of what it needs.

**It requires Settings → Pages → Source set to "GitHub Actions", once, by hand.** Nothing in a
repository can set that, so the first `master` run after this lands will fail its deploy job until
somebody does. Stated here rather than discovered.

Pages deployments take `concurrency: group: pages, cancel-in-progress: false` — the opposite of the
gate's — because cancelling a deployment part-way is how a site ends up half-written, where
cancelling a superseded test run costs nothing.

*Alternative — pushing to a `gh-pages` branch.* Works with the older Pages setting and needs no
manual change, at the cost of a third-party action holding write access to the repository and a
branch of generated HTML living beside the source. The design's preference for few third-party
actions applies most where the action is most privileged.

### Broken documentation links are a gate, and gates live in `check.sh`

This follows from the first decision rather than being a separate opinion. `cargo doc` with
`RUSTDOCFLAGS=-D warnings` is a lint; the workflow runs `check.sh` and nothing else; therefore the
lint goes in `check.sh` and CI acquires it for free. A developer also sees it before pushing, which
is where a broken link is cheapest to fix.

It earns its place rather than being tidiness. Running it now fails, and four of the eight
complaints are stale prose: `Cmd::Start` is documented in two modules as the way to begin something
that no longer has it, `ScopedLink` was deleted in `link-parameterisation` while `perfect_link` still
explains its relationship to it, and `Link::delivered` was renamed `classify`. `CLAUDE.md` treats a
stale docstring as a defect — "a stale quote asserts an algorithm the code deliberately does not
implement" — and this is the mechanical check for the class.

The eight are not one problem. Two are removed items, one is a rename, one is a deleted type, three
are unresolved paths, and one is public documentation pointing at a private field. Each needs reading
rather than a bulk edit, and the fix for a stale reference is to correct the *prose*, not to delete
the link and leave a sentence about something that is not there.

### Concurrency, permissions, and a timeout

Superseded runs on the same ref are cancelled, so pushing twice to a PR does not pay for both.
`permissions: contents: read` — the job reads the repository and needs nothing else, and the default
token grants more than that. A job timeout, because the suite is seconds and a job running for hours
is a hung runner rather than a slow test.

### Two badges, and they say different kinds of thing

The **gate** badge is the workflow's own, so it is green when *every* check passes and red when any
fails — clippy, the ordered-maps guard, the transport guard, as much as the tests. That is the right
granularity: "do not commit until these are clean" already treats them as one thing, and a badge
reporting only `cargo test` would sit green while the determinism guard was failing. It tracks
`master`, which is what a badge should mean.

The **documentation** badge is not a status at all — it is a link wearing the same clothes, so that
the two sit together and the way into the published API reference is the first thing on the page.
Worth being honest about in the workflow's comment and in the README's own words: a static
`docs`/`pages` badge is green whether or not the last deployment succeeded, so it must not be read as
one. If that matters later, the deploy workflow has a status badge of its own that could sit beside
it — but two badges either side of one link is more clutter than the question deserves today.

## Risks / Trade-offs

- **Without a pinned toolchain, a clippy release can turn `master` red with no change to the
  repository.** `-D warnings` is a hard gate, so this will happen eventually rather than might. →
  Accepted deliberately; the cost is a lint fix on a day nobody chose, and the alternative constrains
  every local build. `rust-toolchain.toml` is the fix if it becomes annoying, and it is one file.
- **Ubuntu only leaves determinism across platforms an untested claim** — `simulation`'s spec
  requires that a run is fully determined by its seed, and that is still evidence from one machine.
  → Recorded, not fixed. A macOS entry in a matrix is a three-line change when it is wanted.
- **A green badge will mean less than it appears to** until the workflow has actually failed once.
  → The first PR that breaks something is the check, and it is worth deliberately breaking one to
  confirm the gate catches it rather than assuming a workflow that has only ever passed is working.
- **Denying doc warnings makes a compiler upgrade able to break the build**, since rustdoc gains
  lints as rustc does, and nothing here is pinned. → The same trade already accepted for
  `clippy -D warnings`, and accepted for the same reason: a lint that only warns is a lint nobody
  acts on. This repository already runs three custom guards on that principle.
- **The deploy job will fail until Pages is switched to "GitHub Actions".** → It cannot be avoided
  from inside the repository. It is named in the proposal, in this design and in the tasks, and it
  does not affect the gate.

## Migration Plan

1. The eight documentation defects, then `cargo doc` with warnings denied added to `check.sh`. This
   comes first: adding the guard before the fixes leaves the gate red, and the fixes are worth
   reading individually.
2. The workflow's gate job, and a deliberate red run to prove it fails when it should.
3. The documentation job, once Pages is switched over.
4. The badge and the README.

Additive throughout: no crate, dependency, or local build is touched, and deleting the workflow file
returns the repository to where it is now.

## Open Questions

- **A macOS runner**, whose value is not redundancy but the first evidence that a seeded run replays
  identically on another platform and architecture. That is a claim in `openspec/specs/simulation`,
  and it would be checked rather than asserted for the cost of doubling the runs.
- **Validating the OpenSpec artifacts in CI** — `openspec validate --specs --strict` would catch a
  malformed spec on the pull request that introduced it, rather than at archive time. It needs Node
  on the runner, which is why it is not here; archive already runs it, so the gap is only the window
  between writing a spec and archiving its change.
- **`cargo build --locked`**, so a pull request cannot quietly move `Cargo.lock`. It belongs in
  `check.sh` rather than in the workflow, by the first decision above, so it is a change to the gate
  rather than to CI.
- **Whether the published documentation should carry the `docs/` essays** — `bounded-space.md`,
  `conditional-guarantees.md`, `scope-annotated-modules.md` — beside the API reference. They are the
  argument the modules are written against, and a reader arriving at `epoch_consensus` from a search
  engine has no path to them. That is a site rather than a `cargo doc` output, and a separate
  question.

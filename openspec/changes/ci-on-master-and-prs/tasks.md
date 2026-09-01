## 1. The documentation defects, and then the guard

The fixes come before the guard: adding it first leaves the gate red, and each of these wants
reading rather than a bulk edit. The fix for a stale reference is to correct the **prose** — a
sentence explaining this module's relationship to something that no longer exists is still wrong
once the link is deleted.

- [x] 1.1 `Cmd::Start` in `flooding_consensus` and `uniform_reliable_broadcast`: the command was
      removed, and both modules still document it as the way to begin. Establish what begins them
      now and say that instead
- [x] 1.2 `crate::link::ScopedLink` in `perfect_link`: the type was deleted in
      `link-parameterisation`, and `link.rs` records at length why. `perfect_link` still explains
      that it deliberately does not implement it — rewrite the paragraph against what is there
- [x] 1.3 `Link::delivered` in `link.rs`: renamed to `classify`
- [x] 1.4 `Link` in `flooding_consensus` and `probabilistic_broadcast`: unresolved paths, not stale
      claims — point them at `crate::link::Link`
- [x] 1.5 `recon_sim::Sim::delivery_bound` in `perfect_failure_detector`: `recon-protocols` does not
      depend on `recon-sim` outside dev-dependencies, so rustdoc cannot resolve it. Name it in prose
      rather than linking across a dependency that is not there
- [x] 1.6 `epoch_consensus`'s public documentation links to the private `EpochConsensus::aborted`.
      Decide whether the field should be readable or the sentence should not name it. **Neither was
      needed**: `is_aborted` already exists as the public accessor, so the sentence names that
- [x] 1.7 Add `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings` to `check.sh`, with
      a comment saying what class of defect it catches — the same shape as the three guards already
      there. Verify it fails before 1.1–1.6 and passes after. Confirmed by reintroducing one stale
      link: exit 101 with it, 0 without

## 2. The gate on master and pull requests

- [x] 2.1 Add `.github/workflows/ci.yml`: on pushes to `master` and pull requests targeting it, one
      job on `ubuntu-latest` whose build step is `./scripts/check.sh` and nothing that reimplements
      it
- [x] 2.2 `rustup update stable` rather than a toolchain action — the runner already ships rustup,
      clippy and rustfmt, so the only third-party actions are checkout and the cache
- [x] 2.3 Cache the cargo registry and `target/` with `Swatinem/rust-cache`, and check what it
      actually costs: this workspace builds `--all-targets` across thirty-odd test binaries. If
      `target/` approaches the per-repository limit, cache the registry only and say so in the file.
      Left to `Swatinem/rust-cache`'s own defaults, which already exclude the build artefacts of the
      workspace's own crates; the size is worth watching on the first few runs rather than guessing
- [x] 2.4 `concurrency` cancelling superseded runs on the same ref, `permissions: contents: read`,
      and a job timeout
- [x] 2.5 Comment the workflow with *why* it runs the script rather than the checks — the next
      person to add a step is the one who needs to read it

## 3. Prove the gate fails

- [ ] 3.1 Open a pull request that deliberately breaks one check — a formatting violation is the
      cheapest — and confirm CI goes red and the log names which check failed
- [ ] 3.2 Confirm a run reports **every** failing check rather than stopping at the first: break two
      at once and read the summary. `check.sh` aggregates by design, and a workflow that lost that
      would be worse than the script it replaces
- [ ] 3.3 Confirm the gate badge goes red, and green again when the branch is fixed. A badge that
      has only ever been green is a badge nobody has tested

## 4. Publishing the documentation

- [ ] 4.1 **Set Settings → Pages → Source to "GitHub Actions".** Manual, once, and nothing in the
      repository can do it — the deploy job fails until it is done
- [x] 4.2 A second job, `needs` the gate and `if: github.ref == 'refs/heads/master'`, building
      `cargo doc --workspace --no-deps`. A pull request builds no documentation and deploys none
- [x] 4.3 Write the landing page `cargo doc` does not: a small `index.html` naming the three crates
      and how they relate — core, then the simulator, then the protocols — rather than redirecting
      to whichever one seems most important. Lives in `.github/pages/`, not `docs/`: that directory
      holds the project's essays and has a row per file in the README's documentation table, and a
      piece of site scaffolding is not one of them
- [x] 4.4 `actions/upload-pages-artifact` and `actions/deploy-pages`, with `pages: write` and
      `id-token: write` on this job **only**, so the gate job keeps `contents: read`
- [x] 4.5 `concurrency: group: pages, cancel-in-progress: false` — the opposite of the gate's,
      because cancelling a deployment part-way leaves a half-written site where cancelling a
      superseded test run costs nothing
- [ ] 4.6 Verify the published site: the three crates reachable from the landing page, a module's
      quoted pseudocode rendering as intended, and the intra-doc links resolving now that 1.1–1.6
      are fixed

## 5. What this dates

- [x] 5.1 Two badges at the head of `README.md`: the workflow's own for the gate, and a link to the
      published documentation. Say in the surrounding text that the second is a link rather than a
      status, so a reader does not take a green `docs` badge as evidence the last deploy worked
- [x] 5.2 The guard table: it describes checks and says nothing about what runs them. Add the
      documentation guard as a row, say that CI runs the whole gate on `master` and on every pull
      request, and that the badge reports all of it rather than the tests alone
- [x] 5.3 The build instructions, which present `./scripts/check.sh` as something to remember to run
- [x] 5.4 Point at the published API documentation where the README introduces the crates, since
      that is where a reader wanting it will be
- [ ] 5.5 `./scripts/check.sh` passes locally — including its new check — and the workflow passes on
      a pull request. The two being the same run is the point of the change.
      **Local half done**: all eight checks pass. The CI half needs the workflow on GitHub

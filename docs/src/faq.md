# FAQ

**A file is reported `unused`, but a framework loads it.** kndo reaches files
from roots: manifest entries and scripts, launchers, conventions, and what
extensions declare. Ask `kndo trace <path>` to see whether anything reaches it
and `kndo used-by <path>` for what keeps it alive. If the framework's rule is
statable — a directory it scans, an annotation it dispatches on, a name a
manifest declares — an [extension](plugins.md) states it once for every
project. If it is a one-off, a `kndo:allow unused -- reason` in the file is the
honest record.

**Why is the report identical with `--threads 1` and with 32 threads?** Because
determinism is a gate: order-dependent logic consumes sorted inputs, analysis
runs on evidence, and wall-clock time never enters the engine. Two runs that
differ are a bug to report.

**What is in `.kndo/`?** `cache/` — the persisted evidence and graph, safe to
delete, worth caching in CI; `baseline.json` — the accepted findings, worth
committing; `plugins/` — WASM components kndo loads as extensions. Ignore the
cache in git, commit the baseline.

**Why does `untested` say `probable` on a file rather than `certain` on a
function?** Without a coverage report the graph is the evidence: a file no test
reaches is probably untested, and everything a test imports — however
indirectly — counts as exercised. Drop a coverage report from your test run at
one of the [conventional paths](health.md) and the same analysis judges per
function, `certain`, from what actually executed.

**What is an abstention?** An analysis saying it cannot judge, in the report:
no coverage ingested, no test roots anywhere, files a manifest's ecosystem
could import that no adapter claims. Abstaining is the alternative to guessing,
and an abstention never lowers health.

**Is health a score?** No. It is `implicated / subjects`, rendered as a
percentage of the judged graph that findings do not touch. No weights, no
history, no clock; the baseline does not raise it, `kndo:allow` does.

**Can I see the raw evidence for a finding?** `kndo explain <id>` prints the
finding and its subject as the graph sees it, with the next verbs to run.

**Does kndo read my code anywhere else?** No. It runs where you run it, reads
the tree, and writes `.kndo/`. The GitHub Action publishes the report to the
pull request through the token you give it.

**Windows?** Tested in CI on every push; no release archive yet — build from
source.

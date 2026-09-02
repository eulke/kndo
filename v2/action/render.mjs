// kndo-action's publishing half: reads the JSON report — the only contract this
// frontend consumes (schemas/report.schema.json) — and publishes it to three
// surfaces: the sticky PR comment (upserted in place, marked by a hidden HTML
// comment), file annotations for findings at warning or above (capped at
// GitHub's per-step limit), and the job summary (always, even where a comment is
// impossible). Zero dependencies; node 20+.
//
// Env: KNDO_REPORT (path), KNDO_EXIT, KNDO_FAIL_ON, KNDO_COMMENT ("true"/"false"),
// GITHUB_TOKEN, plus the standard GitHub runner environment.
import { readFileSync, appendFileSync } from "node:fs";

const MARKER = "<!-- kndo-report -->";
const ANNOTATION_CAP = 10; // GitHub renders ~10 annotations per step; honored explicitly.
const SECTION_CAP = 30; // hard cap per comment section, with a link to the run for the rest.

const report = JSON.parse(readFileSync(process.env.KNDO_REPORT, "utf8"));
const exitCode = Number(process.env.KNDO_EXIT ?? "0");

// Severity outranks everything in the display order, as in every kndo render.
const SEVERITY_RANK = { error: 0, warning: 1, info: 2 };
const GLYPH = { error: "✗", warning: "▲", info: "·" };

const findings = report.findings ?? [];
const fixed = report.fixed ?? [];
const diffMode = (report.run?.mode ?? "full") !== "full";

/// The subject as a reader locates it: `path:line` for files, symbols and
/// imports; the manifest and name for a dependency.
function where(f) {
  const s = f.subject ?? {};
  if (s.kind === "dependency") return `${s.owner_manifest} → ${s.name}`;
  const line = f.lines?.start;
  return line ? `${s.path}:${line}` : (s.path ?? "—");
}

function subjectName(f) {
  const s = f.subject ?? {};
  if (s.kind === "symbol") {
    const sel = s.selector ?? {};
    if (typeof sel.Free === "string") return sel.Free;
    if (sel.Member) return `${sel.Member.owner}.${sel.Member.name}`;
  }
  if (s.kind === "dependency") return s.name;
  if (s.kind === "import") return s.specifier;
  return s.path ?? "";
}

function mdEscape(s) {
  return String(s).replace(/\|/g, "\\|").replace(/\n/g, " ");
}

function findingRow(f) {
  const glyph = GLYPH[f.severity] ?? "•";
  const label = `\`${f.category}\` ${f.subject?.kind ?? ""} (${f.confidence})`;
  return `| ${glyph} | ${label} | \`${mdEscape(where(f))}\` | ${mdEscape(f.message)} |`;
}

/// `100 × (1 − implicated/subjects)`, one decimal — the same arithmetic every
/// kndo frontend prints.
function scoreText(h) {
  if (!h || !h.subjects) return "100.0";
  return (100 * (1 - h.implicated / h.subjects)).toFixed(1);
}

function healthLine() {
  const h = report.health;
  if (!h) return "";
  const now = `${scoreText(h)} (${h.implicated} of ${h.subjects} implicated)`;
  const base = report.base_health;
  return base ? `health ${scoreText(base)} → ${now}` : `health ${now}`;
}

function buildMarkdown() {
  const runUrl = `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`;
  const parts = [];
  const headBits = [];
  if (diffMode) {
    headBits.push(`${findings.length} new`, `${fixed.length} fixed`);
  } else {
    headBits.push(`${findings.length} finding${findings.length === 1 ? "" : "s"}`);
  }
  const health = healthLine();
  if (health) headBits.push(health);
  parts.push(`### kndo · ${headBits.join(" · ")}`);

  if (findings.length > 0) {
    const sorted = [...findings].sort(
      (a, b) =>
        (SEVERITY_RANK[a.severity] ?? 9) - (SEVERITY_RANK[b.severity] ?? 9) ||
        a.category.localeCompare(b.category) ||
        a.id.localeCompare(b.id),
    );
    const shown = sorted.slice(0, SECTION_CAP);
    parts.push(
      diffMode ? "**New**" : "**Findings**",
      "| | finding | where | why |",
      "|-|---------|-------|-----|",
      ...shown.map(findingRow),
    );
    if (sorted.length > shown.length) {
      parts.push(`…and ${sorted.length - shown.length} more — [full report in the workflow run](${runUrl})`);
    }
  } else if (diffMode) {
    parts.push("No new findings.");
  }

  if (fixed.length > 0) {
    const shown = fixed.slice(0, SECTION_CAP);
    // Fixed findings always render — the reward loop applies to reviewers too.
    parts.push(
      "**Fixed** " +
        shown.map((f) => `✓ \`${f.category}\` \`${mdEscape(subjectName(f) || where(f))}\``).join(" · "),
    );
    if (fixed.length > shown.length) parts.push(`…and ${fixed.length - shown.length} more fixed`);
  }

  const baselined = report.baselined ?? 0;
  if (baselined > 0) {
    parts.push(`<details><summary>${baselined} baseline finding${baselined === 1 ? "" : "s"} unchanged</summary>\n\nAcknowledged at adoption time (\`.kndo/baseline.json\`); not shown here.\n</details>`);
  }

  const abstained = report.abstained ?? [];
  if (abstained.length > 0) {
    parts.push(
      `<details><summary>${abstained.length} abstention${abstained.length === 1 ? "" : "s"} — what this run did not judge</summary>\n`,
      ...abstained.slice(0, SECTION_CAP).map((a) => `- \`${a.category}\`: ${mdEscape(typeof a.reason === "string" ? a.reason : JSON.stringify(a.reason))}`),
      "</details>",
    );
  }

  const diagnostics = report.diagnostics ?? [];
  if (diagnostics.length > 0) {
    parts.push(
      `<details><summary>${diagnostics.length} diagnostic${diagnostics.length === 1 ? "" : "s"}</summary>\n`,
      ...diagnostics.slice(0, SECTION_CAP).map((d) => `- ${d.path ? `\`${mdEscape(d.path)}\`: ` : ""}${mdEscape(d.message)}`),
      "</details>",
    );
  }

  const suppressed = report.suppressed?.total ?? 0;
  if (suppressed > 0) {
    parts.push(`_${suppressed} finding${suppressed === 1 ? "" : "s"} suppressed by pragma_`);
  }

  parts.push("", `<sub>[run](${runUrl}) · mode \`${report.run?.mode}\` · fail-on \`${process.env.KNDO_FAIL_ON}\` · report ${report.run?.schema}</sub>`);
  parts.push(MARKER);
  return parts.join("\n");
}

function emitAnnotations() {
  const severe = findings.filter(
    (f) => (f.severity === "warning" || f.severity === "error") && f.subject?.path,
  );
  for (const f of severe.slice(0, ANNOTATION_CAP)) {
    const line = f.lines?.start ?? 1;
    const endLine = f.lines?.end ?? line;
    const level = f.severity === "error" ? "error" : "warning";
    // Workflow-command message: escape %, \r, \n per the commands spec.
    const msg = String(f.message).replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
    console.log(
      `::${level} file=${f.subject.path},line=${line},endLine=${endLine},title=kndo ${f.category}::${msg}`,
    );
  }
  if (severe.length > ANNOTATION_CAP) {
    console.log(
      `::notice title=kndo::${severe.length - ANNOTATION_CAP} more finding(s) not annotated (GitHub per-step cap) — see the PR comment / job summary`,
    );
  }
}

async function upsertComment(body) {
  if (process.env.KNDO_COMMENT !== "true") return;
  let event = {};
  try {
    event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
  } catch {
    /* no event payload — not a workflow context that can comment */
  }
  const pr = event.pull_request?.number;
  if (!pr) return; // not a PR run — annotations + summary already cover it.
  const api = process.env.GITHUB_API_URL ?? "https://api.github.com";
  const repo = process.env.GITHUB_REPOSITORY;
  const headers = {
    Authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
    Accept: "application/vnd.github+json",
    "User-Agent": "kndo-action",
  };
  try {
    let existing = null;
    for (let page = 1; page <= 5 && !existing; page++) {
      const res = await fetch(`${api}/repos/${repo}/issues/${pr}/comments?per_page=100&page=${page}`, { headers });
      if (!res.ok) throw new Error(`list comments: HTTP ${res.status}`);
      const comments = await res.json();
      existing = comments.find((c) => typeof c.body === "string" && c.body.includes(MARKER)) ?? null;
      if (comments.length < 100) break;
    }
    const res = existing
      ? await fetch(`${api}/repos/${repo}/issues/comments/${existing.id}`, {
          method: "PATCH",
          headers,
          body: JSON.stringify({ body }),
        })
      : await fetch(`${api}/repos/${repo}/issues/${pr}/comments`, {
          method: "POST",
          headers,
          body: JSON.stringify({ body }),
        });
    if (!res.ok) throw new Error(`write comment: HTTP ${res.status}`);
  } catch (e) {
    // Fork PRs: the default token cannot write comments — degrade, never fail.
    const notice = `kndo: could not publish the PR comment (${e.message}) — likely a fork PR without a write token; the report is in this job summary instead.`;
    console.log(`::notice title=kndo::${notice}`);
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n> ${notice}\n`);
  }
}

const markdown = buildMarkdown();
appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown + "\n");
if (exitCode !== 2) emitAnnotations();
await upsertComment(markdown);
